//! Kubernetes driver: one pod per live session.
//!
//! The pod runs the harness profile's image, or an environment's image with
//! the agent carried in from the harness image. Init containers do the
//! carrying: the runner binary always, the agent (`/opt/harness`) when there
//! is an environment, and whatever the image keeps in the home directory,
//! which the pod's own writable home would otherwise hide.

use super::{
    ContainerInfo, EventInfo, Inspection, LaunchRequest, NodeImage, NodeInfo, PullInfo, PullState, SandboxDriver, SandboxInfo,
    SandboxState, normalize_image,
};
use crate::config::Config;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hyperbeam_core::protocol::HARNESS_DIR;
use k8s_openapi::api::core::v1::{ContainerStatus, Event, Node, Pod, Secret};
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::{Client, ResourceExt};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const SESSION_LABEL: &str = "hyperbeam.dev/session";
/// On image pull pods: which image and node they are for, as a short hash.
const PULL_LABEL: &str = "hyperbeam.dev/pull";
/// On image pull pods: the image as it was asked for.
const IMAGE_ANNOTATION: &str = "hyperbeam.dev/image";
/// A finished pull stays on the Cluster page this long; then its pod goes.
const PULL_KEEP_SECS: i64 = 10 * 60;
/// A pull still going after this long is given up on.
const PULL_GIVE_UP_SECS: i64 = 60 * 60;
const RUNNER_PATH: &str = "/hyperbeam/bin/hyperbeam-runner";
const HOME: &str = "/home/agent";
const HOME_SIZE: &str = "2Gi";
/// The most an image's home directory may hold and still be carried into the
/// sandbox's home, which is limited to `HOME_SIZE`.
const HOME_SEED_BYTES: u64 = 1 << 30;
/// A kubelet reports at most this many of its node's images unless it is
/// configured otherwise, largest first.
const NODE_IMAGE_REPORT_LIMIT: usize = 50;
/// A pod waits briefly for its token Secret; past this it is stuck.
const CONFIG_GRACE_SECS: i64 = 60;
/// The most recent events kept for the Cluster page.
const EVENT_LIMIT: usize = 100;

/// How agent pods are shaped; separate from the client so it can be tested.
#[derive(Clone)]
pub struct PodSettings {
    pub runner_image: String,
    pub pull_policy: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub workspace_size: String,
}

pub struct KubernetesDriver {
    client: Client,
    namespace: String,
    settings: PodSettings,
}

impl KubernetesDriver {
    pub async fn new(cfg: &Config) -> Result<Self> {
        let client = Client::try_default()
            .await
            .context("could not configure a Kubernetes client (in-cluster config or kubeconfig)")?;
        Ok(Self {
            client,
            namespace: cfg.k8s_namespace.clone(),
            settings: PodSettings {
                runner_image: cfg.runner_image.clone(),
                pull_policy: cfg.k8s_image_pull_policy.clone(),
                cpu_limit: cfg.k8s_cpu_limit.clone(),
                memory_limit: cfg.k8s_memory_limit.clone(),
                workspace_size: cfg.k8s_workspace_size.clone(),
            },
        })
    }
}

fn pod_name(session_id: Uuid) -> String {
    format!("agent-{session_id}")
}

pub fn pod_manifest(settings: &PodSettings, request: &LaunchRequest) -> serde_json::Value {
    let name = pod_name(request.session_id);
    let mut env = vec![
        json!({ "name": "HYPERBEAM_SERVER_URL", "value": request.server_url }),
        json!({ "name": "HYPERBEAM_RUN_TOKEN", "valueFrom": { "secretKeyRef": { "name": name, "key": "token" } } }),
        json!({ "name": "HYPERBEAM_WORKSPACE", "value": "/workspace" }),
        json!({ "name": "HOME", "value": HOME }),
    ];
    for (key, value) in &request.env {
        env.push(json!({ "name": key, "value": value }));
    }
    let hardened = json!({
        "allowPrivilegeEscalation": false,
        "capabilities": { "drop": ["ALL"] },
    });
    let runner_bin = json!({ "name": "hyperbeam-bin", "mountPath": "/hyperbeam/bin", "readOnly": true });
    // Copies one directory of an image into a volume, with the runner binary.
    let seed = |name: &str, image: &str, from: &str, volume: &str, extra: &[String]| {
        let to = format!("/hyperbeam/{volume}");
        let mut command = vec![RUNNER_PATH.to_string(), "seed".to_string(), from.to_string(), to.clone()];
        command.extend_from_slice(extra);
        json!({
            "name": name,
            "image": image,
            "imagePullPolicy": settings.pull_policy,
            "command": command,
            "volumeMounts": [runner_bin, { "name": volume, "mountPath": to }],
            "securityContext": hardened,
            "resources": { "requests": { "cpu": "100m", "memory": "64Mi" }, "limits": { "cpu": "1", "memory": "256Mi" } },
        })
    };

    let mut init = vec![json!({
        "name": "runner",
        "image": settings.runner_image,
        "imagePullPolicy": settings.pull_policy,
        "command": ["/hyperbeam-runner", "install", RUNNER_PATH],
        "volumeMounts": [{ "name": "hyperbeam-bin", "mountPath": "/hyperbeam/bin" }],
        "securityContext": hardened,
        "resources": { "limits": { "cpu": "200m", "memory": "64Mi" } },
    })];
    let mut mounts = vec![
        runner_bin.clone(),
        json!({ "name": "workspace", "mountPath": "/workspace" }),
        json!({ "name": "home", "mountPath": HOME }),
        json!({ "name": "tmp", "mountPath": "/tmp" }),
    ];
    let mut volumes = vec![
        json!({ "name": "hyperbeam-bin", "emptyDir": {} }),
        json!({ "name": "workspace", "emptyDir": { "sizeLimit": settings.workspace_size } }),
        json!({ "name": "home", "emptyDir": { "sizeLimit": HOME_SIZE } }),
        json!({ "name": "tmp", "emptyDir": { "sizeLimit": "2Gi" } }),
    ];
    if let Some(harness_image) = &request.harness_image {
        // The sandbox runs an environment's image: bring the agent to it.
        init.push(seed("harness", harness_image, HARNESS_DIR, "harness", &[]));
        mounts.push(json!({ "name": "harness", "mountPath": HARNESS_DIR, "readOnly": true }));
        volumes.push(json!({ "name": "harness", "emptyDir": { "sizeLimit": "4Gi" } }));
    }
    // The home volume hides the image's own home directory, so what the image
    // keeps there (a toolchain installed for the user, dotfiles) is copied in.
    init.push(seed("home", &request.image, HOME, "home", &["--max-bytes".to_string(), HOME_SEED_BYTES.to_string()]));

    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "labels": {
                "app.kubernetes.io/managed-by": "hyperbeam",
                "hyperbeam.dev/role": "agent",
                SESSION_LABEL: request.session_id.to_string(),
            },
        },
        "spec": {
            "restartPolicy": "Never",
            "automountServiceAccountToken": false,
            "enableServiceLinks": false,
            "terminationGracePeriodSeconds": 10,
            "securityContext": {
                "runAsNonRoot": true,
                "runAsUser": 1000,
                "runAsGroup": 1000,
                "fsGroup": 1000,
                "seccompProfile": { "type": "RuntimeDefault" },
            },
            "initContainers": init,
            "containers": [{
                "name": "agent",
                "image": request.image,
                "imagePullPolicy": settings.pull_policy,
                "command": [RUNNER_PATH, "run"],
                "workingDir": "/workspace",
                "env": env,
                "volumeMounts": mounts,
                "securityContext": hardened,
                "resources": {
                    "requests": { "cpu": "250m", "memory": "512Mi" },
                    "limits": { "cpu": settings.cpu_limit, "memory": settings.memory_limit },
                },
            }],
            "volumes": volumes,
        },
    })
}

fn list_error(what: &str, err: &kube::Error) -> String {
    match err {
        kube::Error::Api(e) if e.code == 403 => {
            format!("Hyperbeam's service account is not allowed to list {what}. Upgrade the Helm chart, which grants it.")
        }
        _ => format!("could not list {what}: {err}"),
    }
}

/// Bytes in a Kubernetes quantity such as `8052292Ki` or `16G`.
fn quantity_bytes(quantity: &str) -> Option<u64> {
    let split = quantity.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(quantity.len());
    let (number, unit) = quantity.split_at(split);
    let factor: f64 = match unit {
        "" => 1.0,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "Ki" => 1024.0,
        "Mi" => 1024.0 * 1024.0,
        "Gi" => 1024.0 * 1024.0 * 1024.0,
        "Ti" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number.parse::<f64>().ok()? * factor) as u64)
}

pub fn node_info(node: &Node) -> NodeInfo {
    let status = node.status.as_ref();
    let system = status.and_then(|s| s.node_info.as_ref());
    let conditions = status.and_then(|s| s.conditions.as_deref()).unwrap_or_default();
    let ready = conditions.iter().any(|c| c.type_ == "Ready" && c.status == "True");
    let mut problems = Vec::new();
    for condition in conditions {
        let message = condition.message.as_deref().or(condition.reason.as_deref()).unwrap_or_default();
        match condition.type_.as_str() {
            "Ready" if condition.status != "True" => problems.push(format!("Not ready: {message}")),
            "MemoryPressure" | "DiskPressure" | "PIDPressure" | "NetworkUnavailable" if condition.status == "True" => {
                problems.push(format!("{}: {message}", condition.type_));
            }
            _ => {}
        }
    }
    if !conditions.iter().any(|c| c.type_ == "Ready") {
        problems.push("Not ready: the node has not reported its status".to_string());
    }
    let images: Vec<NodeImage> = status
        .and_then(|s| s.images.as_deref())
        .unwrap_or_default()
        .iter()
        .map(|image| NodeImage {
            names: image.names.clone().unwrap_or_default(),
            size_bytes: image.size_bytes.and_then(|bytes| u64::try_from(bytes).ok()),
        })
        .collect();
    let allocatable = |key: &str| status.and_then(|s| s.allocatable.as_ref()).and_then(|a| a.get(key)).map(|q| q.0.clone());
    let mut roles: Vec<String> =
        node.labels().keys().filter_map(|label| label.strip_prefix("node-role.kubernetes.io/")).map(str::to_string).collect();
    roles.sort();
    NodeInfo {
        name: node.name_any(),
        ready,
        schedulable: !node.spec.as_ref().and_then(|s| s.unschedulable).unwrap_or(false),
        roles,
        arch: system.map(|i| i.architecture.clone()).unwrap_or_default(),
        os: system.map(|i| i.os_image.clone()).unwrap_or_default(),
        kubelet: system.map(|i| i.kubelet_version.clone()).unwrap_or_default(),
        runtime: system.map(|i| i.container_runtime_version.clone()).unwrap_or_default(),
        cpu: allocatable("cpu").unwrap_or_default(),
        memory_bytes: allocatable("memory").as_deref().and_then(quantity_bytes),
        problems,
        created_at: node.metadata.creation_timestamp.as_ref().map(|t| t.0),
        images_complete: images.len() < NODE_IMAGE_REPORT_LIMIT,
        images,
    }
}

fn container_info(status: &ContainerStatus, init: bool) -> ContainerInfo {
    let state = status.state.as_ref();
    let (state, reason, message, exit_code) = if let Some(terminated) = state.and_then(|s| s.terminated.as_ref()) {
        ("terminated", terminated.reason.clone(), terminated.message.clone(), Some(terminated.exit_code))
    } else if state.is_some_and(|s| s.running.is_some()) {
        ("running", None, None, None)
    } else {
        let waiting = state.and_then(|s| s.waiting.as_ref());
        ("waiting", waiting.and_then(|w| w.reason.clone()), waiting.and_then(|w| w.message.clone()), None)
    };
    ContainerInfo {
        name: status.name.clone(),
        image: status.image.clone(),
        init,
        state,
        reason: reason.unwrap_or_default(),
        message: message.unwrap_or_default(),
        exit_code,
    }
}

/// What one agent pod is doing, and what is wrong with it if anything is.
pub fn sandbox_info(pod: &Pod, now: DateTime<Utc>) -> SandboxInfo {
    let status = pod.status.as_ref();
    let spec = pod.spec.as_ref();
    let created_at = pod.metadata.creation_timestamp.as_ref().map(|t| t.0);
    let init_statuses = status.and_then(|s| s.init_container_statuses.as_deref()).unwrap_or_default();
    let main_statuses = status.and_then(|s| s.container_statuses.as_deref()).unwrap_or_default();
    let mut containers: Vec<ContainerInfo> = init_statuses.iter().map(|c| container_info(c, true)).collect();
    containers.extend(main_statuses.iter().map(|c| container_info(c, false)));
    // A status names the image as the runtime resolved it; before there is
    // one, and for what the pod was asked to run, the spec is the source.
    let wanted = |name: &str, init: bool| {
        let list = if init { spec.and_then(|s| s.init_containers.as_deref()) } else { spec.map(|s| s.containers.as_slice()) };
        list.unwrap_or_default().iter().find(|c| c.name == name).and_then(|c| c.image.clone())
    };
    for container in &mut containers {
        if let Some(image) = wanted(&container.name, container.init) {
            container.image = image;
        }
    }

    let old = created_at.is_some_and(|created| (now - created).num_seconds() > CONFIG_GRACE_SECS);
    let broken = |c: &ContainerInfo| match c.state {
        "waiting" => {
            c.pull_failed()
                || matches!(c.reason.as_str(), "CrashLoopBackOff" | "CreateContainerError" | "RunContainerError")
                || (c.reason == "CreateContainerConfigError" && old)
        }
        "terminated" => c.exit_code != Some(0),
        _ => false,
    };
    let phase = status.and_then(|s| s.phase.as_deref()).unwrap_or("Unknown");
    let unschedulable = status
        .and_then(|s| s.conditions.as_deref())
        .unwrap_or_default()
        .iter()
        .find(|c| c.type_ == "PodScheduled" && c.status == "False" && c.reason.as_deref() == Some("Unschedulable"));

    let (state, reason, message) = if pod.metadata.deletion_timestamp.is_some() {
        (SandboxState::Terminating, String::new(), String::new())
    } else if let Some(container) = containers.iter().find(|c| broken(c)) {
        let detail = match (&container.message, container.exit_code) {
            (message, _) if !message.is_empty() => message.clone(),
            (_, Some(code)) => format!("exited with code {code}"),
            _ => "could not start".to_string(),
        };
        let reason = if container.reason.is_empty() { "Error".to_string() } else { container.reason.clone() };
        (SandboxState::Failing, reason, format!("{}: {detail}", container.name))
    } else if phase == "Failed" {
        let reason = status.and_then(|s| s.reason.clone()).unwrap_or_else(|| "Failed".to_string());
        (SandboxState::Failing, reason, status.and_then(|s| s.message.clone()).unwrap_or_default())
    } else if phase == "Succeeded" {
        (SandboxState::Finished, String::new(), String::new())
    } else if let Some(condition) = unschedulable {
        (SandboxState::Failing, "Unschedulable".to_string(), condition.message.clone().unwrap_or_default())
    } else if phase == "Running" && !main_statuses.is_empty() && main_statuses.iter().all(|c| c.ready) {
        (SandboxState::Running, String::new(), String::new())
    } else {
        // Whichever step it has reached: the first that has not completed.
        match containers.iter().find(|c| c.state != "terminated") {
            Some(container) if container.init => {
                (SandboxState::Starting, container.reason.clone(), format!("preparing ({})", container.name))
            }
            Some(container) => (SandboxState::Starting, container.reason.clone(), String::new()),
            None if spec.and_then(|s| s.node_name.as_ref()).is_none() => {
                (SandboxState::Starting, "Scheduling".to_string(), "waiting for a node".to_string())
            }
            None => (SandboxState::Starting, String::new(), String::new()),
        }
    };

    SandboxInfo {
        id: pod.labels().get(SESSION_LABEL).and_then(|id| id.parse().ok()),
        name: pod.name_any(),
        node: spec.and_then(|s| s.node_name.clone()),
        state,
        reason,
        message,
        image: spec.and_then(|s| s.containers.first()).and_then(|c| c.image.clone()).unwrap_or_default(),
        restarts: init_statuses.iter().chain(main_statuses).map(|c| c.restart_count).sum(),
        created_at,
        containers,
    }
}

/// Which pull pods are for the same image on the same node.
fn pull_key(image: &str, node: &str) -> String {
    let digest = Sha256::digest(format!("{}\n{node}", normalize_image(image)));
    hex::encode(&digest[..8])
}

/// A pod that only makes `node` fetch `image` again. Its one container runs
/// the image with `imagePullPolicy: Always`, which is what makes the kubelet
/// ask the registry, and does nothing but print the runner's version.
pub fn pull_pod_manifest(settings: &PodSettings, image: &str, node: &str) -> serde_json::Value {
    let key = pull_key(image, node);
    let suffix = &Uuid::new_v4().simple().to_string()[..6];
    let hardened = json!({ "allowPrivilegeEscalation": false, "capabilities": { "drop": ["ALL"] } });
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": format!("pull-{key}-{suffix}"),
            "labels": {
                "app.kubernetes.io/managed-by": "hyperbeam",
                "hyperbeam.dev/role": "image-pull",
                PULL_LABEL: key,
            },
            "annotations": { IMAGE_ANNOTATION: image },
        },
        "spec": {
            "nodeName": node,
            "restartPolicy": "Never",
            "automountServiceAccountToken": false,
            "enableServiceLinks": false,
            "terminationGracePeriodSeconds": 0,
            "securityContext": {
                "runAsNonRoot": true,
                "runAsUser": 1000,
                "runAsGroup": 1000,
                "seccompProfile": { "type": "RuntimeDefault" },
            },
            "initContainers": [{
                "name": "runner",
                "image": settings.runner_image,
                "imagePullPolicy": settings.pull_policy,
                "command": ["/hyperbeam-runner", "install", RUNNER_PATH],
                "volumeMounts": [{ "name": "hyperbeam-bin", "mountPath": "/hyperbeam/bin" }],
                "securityContext": hardened,
                "resources": { "limits": { "cpu": "200m", "memory": "64Mi" } },
            }],
            "containers": [{
                "name": "pull",
                "image": image,
                "imagePullPolicy": "Always",
                "command": [RUNNER_PATH, "--version"],
                "volumeMounts": [{ "name": "hyperbeam-bin", "mountPath": "/hyperbeam/bin", "readOnly": true }],
                "securityContext": hardened,
                "resources": { "limits": { "cpu": "200m", "memory": "64Mi" } },
            }],
            "volumes": [{ "name": "hyperbeam-bin", "emptyDir": {} }],
        },
    })
}

/// How far a pull pod has got. The image is on the node once its container
/// has been created, whatever the container then did.
pub fn pull_info(pod: &Pod) -> PullInfo {
    let status = pod.status.as_ref();
    let init = status.and_then(|s| s.init_container_statuses.as_deref()).unwrap_or_default();
    let main = status.and_then(|s| s.container_statuses.as_deref()).unwrap_or_default();
    let pull = main.iter().find(|c| c.name == "pull");
    let failed_pull = init.iter().map(|c| container_info(c, true)).chain(pull.map(|c| container_info(c, false))).find(|c| c.pull_failed());
    let failed_runner = init.iter().map(|c| container_info(c, true)).find(|c| c.state == "terminated" && c.exit_code != Some(0));
    let pulled = pull.is_some_and(|c| {
        let info = container_info(c, false);
        info.state != "waiting" || matches!(info.reason.as_str(), "CreateContainerError" | "RunContainerError")
    });
    let phase = status.and_then(|s| s.phase.as_deref()).unwrap_or_default();

    let (state, message) = if let Some(c) = failed_pull {
        let message = if c.message.is_empty() { c.reason } else { c.message };
        (PullState::Failed, if c.init { format!("the runner image could not be pulled: {message}") } else { message })
    } else if pulled {
        (PullState::Pulled, String::new())
    } else if failed_runner.is_some() {
        (PullState::Failed, "the runner could not be installed".to_string())
    } else if phase == "Failed" {
        (PullState::Failed, status.and_then(|s| s.message.clone().or(s.reason.clone())).unwrap_or_else(|| "the pull pod failed".into()))
    } else {
        (PullState::Pulling, String::new())
    };
    let finished_at = pull
        .and_then(|c| c.state.as_ref()?.terminated.as_ref()?.finished_at.as_ref())
        .map(|t| t.0)
        .or_else(|| pull.and_then(|c| c.state.as_ref()?.running.as_ref()?.started_at.as_ref()).map(|t| t.0));
    PullInfo {
        image: pod.annotations().get(IMAGE_ANNOTATION).cloned().unwrap_or_default(),
        node: pod.spec.as_ref().and_then(|s| s.node_name.clone()).unwrap_or_default(),
        state,
        message,
        started_at: pod.metadata.creation_timestamp.as_ref().map(|t| t.0),
        finished_at: if state == PullState::Pulled { finished_at } else { None },
    }
}

/// Whether a pull pod has served its purpose and can go.
fn pull_expired(info: &PullInfo, now: DateTime<Utc>) -> bool {
    let age = |at: Option<DateTime<Utc>>| at.map(|at| (now - at).num_seconds()).unwrap_or(i64::MAX);
    match info.state {
        PullState::Pulling => age(info.started_at) > PULL_GIVE_UP_SECS,
        _ => age(info.finished_at.or(info.started_at)) > PULL_KEEP_SECS,
    }
}

fn event_info(event: &Event) -> EventInfo {
    let ts = event
        .last_timestamp
        .as_ref()
        .map(|t| t.0)
        .or(event.series.as_ref().and_then(|s| s.last_observed_time.as_ref()).map(|t| t.0))
        .or(event.event_time.as_ref().map(|t| t.0))
        .or(event.first_timestamp.as_ref().map(|t| t.0))
        .or(event.metadata.creation_timestamp.as_ref().map(|t| t.0));
    let object = &event.involved_object;
    EventInfo {
        ts,
        warning: event.type_.as_deref() == Some("Warning"),
        reason: event.reason.clone().unwrap_or_default(),
        object: format!("{}/{}", object.kind.as_deref().unwrap_or_default(), object.name.as_deref().unwrap_or_default()),
        message: event.message.clone().unwrap_or_default(),
        count: event.count.unwrap_or(1),
    }
}

#[async_trait]
impl SandboxDriver for KubernetesDriver {
    fn name(&self) -> &'static str {
        "kubernetes"
    }

    async fn launch(&self, request: &LaunchRequest) -> Result<()> {
        self.destroy(request.session_id).await.ok();
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), &self.namespace);
        let name = pod_name(request.session_id);

        let pod: Pod = serde_json::from_value(pod_manifest(&self.settings, request))?;
        let created = pods.create(&PostParams::default(), &pod).await.context("could not create the agent pod")?;

        // The token lives in a Secret owned by the pod, so deleting the pod
        // garbage-collects it. The pod waits for the Secret to appear.
        let secret: Secret = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": name,
                "labels": {
                    "app.kubernetes.io/managed-by": "hyperbeam",
                    SESSION_LABEL: request.session_id.to_string(),
                },
                "ownerReferences": [{
                    "apiVersion": "v1",
                    "kind": "Pod",
                    "name": name,
                    "uid": created.uid().context("created pod has no uid")?,
                }],
            },
            "type": "Opaque",
            "stringData": { "token": request.token },
        }))?;
        if let Err(err) = secrets.create(&PostParams::default(), &secret).await {
            pods.delete(&name, &DeleteParams::default()).await.ok();
            return Err(err).context("could not create the agent pod's token secret");
        }
        Ok(())
    }

    async fn destroy(&self, session_id: Uuid) -> Result<()> {
        let name = pod_name(session_id);
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), &self.namespace);
        let gone = |err: &kube::Error| matches!(err, kube::Error::Api(e) if e.code == 404);
        if let Err(err) = secrets.delete(&name, &DeleteParams::default()).await
            && !gone(&err) {
                tracing::warn!("could not delete secret {name}: {err}");
            }
        match pods.delete(&name, &DeleteParams::default().grace_period(5)).await {
            Ok(_) => Ok(()),
            Err(err) if gone(&err) => Ok(()),
            Err(err) => Err(err).context("could not delete the agent pod"),
        }
    }

    async fn list(&self) -> Result<Vec<Uuid>> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let list = pods.list(&ListParams::default().labels("hyperbeam.dev/role=agent")).await?;
        Ok(list
            .items
            .iter()
            .filter(|pod| pod.metadata.deletion_timestamp.is_none())
            .filter_map(|pod| pod.labels().get(SESSION_LABEL)?.parse().ok())
            .collect())
    }

    async fn inspect(&self) -> Inspection {
        let nodes: Api<Node> = Api::all(self.client.clone());
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let events: Api<Event> = Api::namespaced(self.client.clone(), &self.namespace);
        let everything = ListParams::default();
        let agents = ListParams::default().labels("hyperbeam.dev/role=agent");
        let pulling = ListParams::default().labels("hyperbeam.dev/role=image-pull");
        let (nodes, sandbox_pods, pull_pods, events) =
            tokio::join!(nodes.list(&everything), pods.list(&agents), pods.list(&pulling), events.list(&everything));

        let mut inspection = Inspection { namespace: Some(self.namespace.clone()), ..Default::default() };
        match nodes {
            Ok(list) => {
                inspection.nodes = list.items.iter().map(node_info).collect();
                inspection.nodes.sort_by(|a, b| a.name.cmp(&b.name));
            }
            Err(err) => inspection.nodes_error = Some(list_error("nodes", &err)),
        }
        match sandbox_pods {
            Ok(list) => {
                let now = Utc::now();
                inspection.sandboxes = list.items.iter().map(|pod| sandbox_info(pod, now)).collect();
                inspection.sandboxes.sort_by_key(|sandbox| std::cmp::Reverse(sandbox.created_at));
            }
            Err(err) => inspection.sandboxes_error = Some(list_error("pods", &err)),
        }
        match events {
            Ok(list) => {
                inspection.events = list.items.iter().map(event_info).collect();
                inspection.events.sort_by_key(|event| std::cmp::Reverse(event.ts));
                inspection.events.truncate(EVENT_LIMIT);
            }
            Err(err) => inspection.events_error = Some(list_error("events", &err)),
        }
        // Pulls: the latest per image and node. Pods that have served their
        // purpose are removed here, as the page that shows them looks.
        if let Ok(list) = pull_pods {
            let now = Utc::now();
            let mut list: Vec<&Pod> = list.items.iter().filter(|pod| pod.metadata.deletion_timestamp.is_none()).collect();
            list.sort_by_key(|pod| std::cmp::Reverse(pod.metadata.creation_timestamp.as_ref().map(|t| t.0)));
            let mut seen = std::collections::HashSet::new();
            for pod in list {
                let info = pull_info(pod);
                let latest = seen.insert(pod.labels().get(PULL_LABEL).cloned().unwrap_or_default());
                if !latest || pull_expired(&info, now) {
                    let (pods, name) = (pods.clone(), pod.name_any());
                    tokio::spawn(async move { pods.delete(&name, &DeleteParams::default()).await.ok() });
                } else {
                    inspection.pulls.push(info);
                }
            }
        }
        inspection
    }

    async fn pull(&self, image: &str) -> Result<usize> {
        let nodes: Api<Node> = Api::all(self.client.clone());
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let nodes = nodes.list(&ListParams::default()).await.map_err(|err| anyhow::anyhow!(list_error("nodes", &err)))?;
        let ready: Vec<String> = nodes.items.iter().map(node_info).filter(|n| n.ready).map(|n| n.name).collect();
        anyhow::ensure!(!ready.is_empty(), "no node is ready to pull onto");
        for node in &ready {
            // An earlier pull of the same image here is superseded, and
            // removed by `inspect`, which shows only the latest.
            let pod: Pod = serde_json::from_value(pull_pod_manifest(&self.settings, image, node))?;
            pods.create(&PostParams::default(), &pod).await.with_context(|| format!("could not start a pull on {node}"))?;
        }
        Ok(ready.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PodSettings {
        PodSettings {
            runner_image: "runner:1".into(),
            pull_policy: "IfNotPresent".into(),
            cpu_limit: "2".into(),
            memory_limit: "4Gi".into(),
            workspace_size: "10Gi".into(),
        }
    }

    fn request(image: &str, harness_image: Option<&str>) -> LaunchRequest {
        LaunchRequest {
            session_id: Uuid::nil(),
            token: "nmr_secret".into(),
            image: image.into(),
            harness_image: harness_image.map(str::to_string),
            server_url: "http://hyperbeam:8080".into(),
            env: Default::default(),
        }
    }

    #[test]
    fn manifest_is_a_valid_restricted_pod() {
        let manifest = pod_manifest(&settings(), &request("harness:1", None));
        // The token is referenced from a Secret, never written into the pod.
        assert!(!manifest.to_string().contains("nmr_secret"));
        let pod: Pod = serde_json::from_value(manifest).expect("manifest matches the Pod schema");
        let spec = pod.spec.unwrap();
        assert_eq!(spec.automount_service_account_token, Some(false));
        assert_eq!(spec.security_context.as_ref().unwrap().run_as_non_root, Some(true));
        let agent = &spec.containers[0];
        assert_eq!(agent.image.as_deref(), Some("harness:1"));
        let context = agent.security_context.as_ref().unwrap();
        assert_eq!(context.allow_privilege_escalation, Some(false));
        assert_eq!(context.capabilities.as_ref().unwrap().drop.as_ref().unwrap(), &["ALL".to_string()]);
        // The runner is installed, then the image's home is carried into the writable one.
        let init = spec.init_containers.unwrap();
        let names: Vec<&str> = init.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["runner", "home"]);
        assert_eq!(init[0].image.as_deref(), Some("runner:1"));
        assert_eq!(init[1].image.as_deref(), Some("harness:1"));
        assert_eq!(
            init[1].command.as_ref().unwrap(),
            &[RUNNER_PATH, "seed", "/home/agent", "/hyperbeam/home", "--max-bytes", "1073741824"].map(String::from)
        );
        for container in &init {
            assert_eq!(container.security_context.as_ref().unwrap().allow_privilege_escalation, Some(false));
        }
    }

    #[test]
    fn an_environment_runs_its_image_with_the_agent_carried_in() {
        let manifest = pod_manifest(&settings(), &request("rust:1", Some("harness:1")));
        let pod: Pod = serde_json::from_value(manifest).expect("manifest matches the Pod schema");
        let spec = pod.spec.unwrap();
        let init = spec.init_containers.unwrap();
        let names: Vec<&str> = init.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["runner", "harness", "home"]);
        // The agent comes out of the harness image; everything else is the environment.
        assert_eq!(init[1].image.as_deref(), Some("harness:1"));
        assert_eq!(init[1].command.as_ref().unwrap(), &[RUNNER_PATH, "seed", "/opt/harness", "/hyperbeam/harness"].map(String::from));
        assert_eq!(init[2].image.as_deref(), Some("rust:1"));
        let agent = &spec.containers[0];
        assert_eq!(agent.image.as_deref(), Some("rust:1"));
        let mount = agent.volume_mounts.as_ref().unwrap().iter().find(|m| m.name == "harness").expect("the agent is mounted");
        assert_eq!((mount.mount_path.as_str(), mount.read_only), ("/opt/harness", Some(true)));
        assert!(spec.volumes.unwrap().iter().any(|v| v.name == "harness"));
    }

    fn pod(status: serde_json::Value) -> Pod {
        let mut manifest = pod_manifest(&settings(), &request("harness:1", None));
        manifest["metadata"]["creationTimestamp"] = json!("2026-01-01T00:00:00Z");
        manifest["status"] = status;
        serde_json::from_value(manifest).expect("a pod")
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::seconds(seconds)
    }

    #[test]
    fn an_image_that_cannot_be_pulled_is_a_failing_sandbox() {
        let info = sandbox_info(
            &pod(json!({
                "phase": "Pending",
                "initContainerStatuses": [
                    { "name": "runner", "image": "runner:1", "imageID": "", "ready": true, "restartCount": 0,
                      "state": { "terminated": { "exitCode": 0, "reason": "Completed" } } },
                    { "name": "home", "image": "harness:1", "imageID": "", "ready": false, "restartCount": 0,
                      "state": { "waiting": { "reason": "ImagePullBackOff", "message": "Back-off pulling image \"harness:1\"" } } },
                ],
            })),
            at(5),
        );
        assert_eq!(info.id, Some(Uuid::nil()));
        assert_eq!(info.state, SandboxState::Failing);
        assert_eq!(info.reason, "ImagePullBackOff");
        assert!(info.message.starts_with("home: Back-off pulling image"));
        assert!(info.containers[1].pull_failed() && !info.containers[0].pull_failed());
    }

    #[test]
    fn a_sandbox_is_starting_until_its_agent_is_ready() {
        let waiting = json!({
            "phase": "Pending",
            "initContainerStatuses": [
                { "name": "runner", "image": "runner:1", "imageID": "", "ready": false, "restartCount": 0, "state": { "running": {} } },
            ],
        });
        let info = sandbox_info(&pod(waiting), at(5));
        assert_eq!((info.state, info.message.as_str()), (SandboxState::Starting, "preparing (runner)"));

        // The token Secret is created just after the pod: not a failure at first.
        let config = |seconds| {
            sandbox_info(
                &pod(json!({
                    "phase": "Pending",
                    "containerStatuses": [
                        { "name": "agent", "image": "harness:1", "imageID": "", "ready": false, "restartCount": 0,
                          "state": { "waiting": { "reason": "CreateContainerConfigError", "message": "secret not found" } } },
                    ],
                })),
                at(seconds),
            )
            .state
        };
        assert_eq!(config(5), SandboxState::Starting);
        assert_eq!(config(300), SandboxState::Failing);

        let running = json!({
            "phase": "Running",
            "containerStatuses": [
                { "name": "agent", "image": "harness:1", "imageID": "", "ready": true, "restartCount": 0, "state": { "running": {} } },
            ],
        });
        assert_eq!(sandbox_info(&pod(running), at(5)).state, SandboxState::Running);

        let unschedulable = json!({
            "phase": "Pending",
            "conditions": [{ "type": "PodScheduled", "status": "False", "reason": "Unschedulable", "message": "0/1 nodes are available: 1 Insufficient memory." }],
        });
        let info = sandbox_info(&pod(unschedulable), at(5));
        assert_eq!((info.state, info.reason.as_str()), (SandboxState::Failing, "Unschedulable"));
    }

    #[test]
    fn a_node_reports_its_health_and_images() {
        let node: Node = serde_json::from_value(json!({
            "metadata": { "name": "pi", "labels": { "node-role.kubernetes.io/control-plane": "true" } },
            "status": {
                "allocatable": { "cpu": "4", "memory": "8052292Ki" },
                "conditions": [
                    { "type": "Ready", "status": "True" },
                    { "type": "DiskPressure", "status": "True", "message": "kubelet has disk pressure" },
                    { "type": "EtcdIsVoter", "status": "True" },
                ],
                "nodeInfo": {
                    "architecture": "arm64", "bootID": "", "containerRuntimeVersion": "containerd://2.0", "kernelVersion": "", "kubeProxyVersion": "",
                    "kubeletVersion": "v1.33.1+k3s1", "machineID": "", "operatingSystem": "linux", "osImage": "Debian 12", "systemUUID": "",
                },
                "images": [{ "names": ["ghcr.io/acme/dev@sha256:abc", "ghcr.io/acme/dev:1"], "sizeBytes": 1000 }],
            },
        }))
        .unwrap();
        let info = node_info(&node);
        assert!(info.ready && info.schedulable && info.images_complete);
        assert_eq!(info.roles, ["control-plane"]);
        assert_eq!(info.problems, ["DiskPressure: kubelet has disk pressure"]);
        assert_eq!((info.arch.as_str(), info.memory_bytes), ("arm64", Some(8052292 * 1024)));
        assert_eq!(info.images[0].size_bytes, Some(1000));
    }

    fn pull_pod(status: serde_json::Value) -> Pod {
        let mut manifest = pull_pod_manifest(&settings(), "ghcr.io/acme/dev:latest", "pi");
        manifest["metadata"]["creationTimestamp"] = json!("2026-01-01T00:00:00Z");
        manifest["status"] = status;
        serde_json::from_value(manifest).expect("a pod")
    }

    #[test]
    fn a_pull_pod_fetches_the_image_afresh_on_one_node() {
        let pod = pull_pod(json!({}));
        let spec = pod.spec.as_ref().unwrap();
        assert_eq!(spec.node_name.as_deref(), Some("pi"));
        assert_eq!(spec.security_context.as_ref().unwrap().run_as_non_root, Some(true));
        let pull = &spec.containers[0];
        assert_eq!((pull.image.as_deref(), pull.image_pull_policy.as_deref()), (Some("ghcr.io/acme/dev:latest"), Some("Always")));
        assert_eq!(pull.security_context.as_ref().unwrap().allow_privilege_escalation, Some(false));
        // Not a sandbox: neither listed as one nor cleaned up as an orphan.
        assert_eq!(pod.labels()["hyperbeam.dev/role"], "image-pull");
        assert!(!pod.labels().contains_key(SESSION_LABEL));
        // However the image is spelled, it is the same pull.
        assert_eq!(pull_key("ghcr.io/acme/dev", "pi"), pod.labels()[PULL_LABEL]);
        assert_ne!(pull_key("ghcr.io/acme/dev", "pi2"), pod.labels()[PULL_LABEL]);
    }

    #[test]
    fn a_pull_is_done_once_its_container_exists() {
        let runner_done = json!({ "name": "runner", "image": "runner:1", "imageID": "", "ready": false, "restartCount": 0,
                                  "state": { "terminated": { "exitCode": 0 } } });
        let with = |state: serde_json::Value| {
            pull_info(&pull_pod(json!({
                "phase": "Pending",
                "initContainerStatuses": [runner_done],
                "containerStatuses": [{ "name": "pull", "image": "ghcr.io/acme/dev:latest", "imageID": "", "ready": false, "restartCount": 0, "state": state }],
            })))
        };
        assert_eq!(with(json!({ "waiting": { "reason": "ContainerCreating" } })).state, PullState::Pulling);
        let failed = with(json!({ "waiting": { "reason": "ErrImagePull", "message": "not found" } }));
        assert_eq!((failed.state, failed.message.as_str()), (PullState::Failed, "not found"));
        let pulled = with(json!({ "terminated": { "exitCode": 0, "finishedAt": "2026-01-01T00:01:00Z" } }));
        assert_eq!((pulled.state, pulled.finished_at), (PullState::Pulled, Some(at(60))));
        // An image with nothing the runner can run on is still on the node.
        assert_eq!(with(json!({ "waiting": { "reason": "RunContainerError" } })).state, PullState::Pulled);
        assert_eq!((pulled.image.as_str(), pulled.node.as_str()), ("ghcr.io/acme/dev:latest", "pi"));

        assert!(!pull_expired(&pulled, at(60 + PULL_KEEP_SECS)));
        assert!(pull_expired(&pulled, at(61 + PULL_KEEP_SECS)));
        let pulling = with(json!({ "waiting": { "reason": "ContainerCreating" } }));
        assert!(!pull_expired(&pulling, at(PULL_KEEP_SECS * 2)));
        assert!(pull_expired(&pulling, at(PULL_GIVE_UP_SECS + 1)));
    }
}
