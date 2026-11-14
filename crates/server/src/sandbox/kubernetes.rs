//! Kubernetes driver: one pod per live session.
//!
//! The pod runs the harness profile's image, or an environment's image with
//! the agent carried in from the harness image. Init containers do the
//! carrying: the runner binary always, the agent (`/opt/harness`) when there
//! is an environment, and whatever the image keeps in the home directory,
//! which the pod's own writable home would otherwise hide.

use super::{LaunchRequest, SandboxDriver};
use crate::config::Config;
use anyhow::{Context, Result};
use async_trait::async_trait;
use hyperbeam_core::protocol::HARNESS_DIR;
use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::{Client, ResourceExt};
use serde_json::json;
use uuid::Uuid;

const SESSION_LABEL: &str = "hyperbeam.dev/session";
const RUNNER_PATH: &str = "/hyperbeam/bin/hyperbeam-runner";
const HOME: &str = "/home/agent";
const HOME_SIZE: &str = "2Gi";
/// The most an image's home directory may hold and still be carried into the
/// sandbox's home, which is limited to `HOME_SIZE`.
const HOME_SEED_BYTES: u64 = 1 << 30;

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
}
