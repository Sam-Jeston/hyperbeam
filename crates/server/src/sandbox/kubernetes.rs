//! Kubernetes driver: one pod per live session.
//!
//! The harness image is whatever the profile names. The runner binary is
//! copied in by an init container, so harness images need nothing
//! Hyperbeam-specific beyond `git` and the harness itself.

use super::{LaunchRequest, SandboxDriver};
use crate::config::Config;
use anyhow::{Context, Result};
use async_trait::async_trait;
use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::{Client, ResourceExt};
use serde_json::json;
use uuid::Uuid;

const SESSION_LABEL: &str = "hyperbeam.dev/session";
const RUNNER_PATH: &str = "/hyperbeam/bin/hyperbeam-runner";

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
        json!({ "name": "HOME", "value": "/home/agent" }),
    ];
    for (key, value) in &request.env {
        env.push(json!({ "name": key, "value": value }));
    }
    let hardened = json!({
        "allowPrivilegeEscalation": false,
        "capabilities": { "drop": ["ALL"] },
    });
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
                "name": "agent",
                "image": request.image,
                "imagePullPolicy": settings.pull_policy,
                "command": [RUNNER_PATH, "run"],
                "workingDir": "/workspace",
                "env": env,
                "volumeMounts": [
                    { "name": "hyperbeam-bin", "mountPath": "/hyperbeam/bin", "readOnly": true },
                    { "name": "workspace", "mountPath": "/workspace" },
                    { "name": "home", "mountPath": "/home/agent" },
                    { "name": "tmp", "mountPath": "/tmp" },
                ],
                "securityContext": hardened,
                "resources": {
                    "requests": { "cpu": "250m", "memory": "512Mi" },
                    "limits": { "cpu": settings.cpu_limit, "memory": settings.memory_limit },
                },
            }],
            "volumes": [
                { "name": "hyperbeam-bin", "emptyDir": {} },
                { "name": "workspace", "emptyDir": { "sizeLimit": settings.workspace_size } },
                { "name": "home", "emptyDir": { "sizeLimit": "2Gi" } },
                { "name": "tmp", "emptyDir": { "sizeLimit": "2Gi" } },
            ],
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
        if let Err(err) = secrets.delete(&name, &DeleteParams::default()).await {
            if !gone(&err) {
                tracing::warn!("could not delete secret {name}: {err}");
            }
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

    #[test]
    fn manifest_is_a_valid_restricted_pod() {
        let settings = PodSettings {
            runner_image: "runner:1".into(),
            pull_policy: "IfNotPresent".into(),
            cpu_limit: "2".into(),
            memory_limit: "4Gi".into(),
            workspace_size: "10Gi".into(),
        };
        let request = LaunchRequest {
            session_id: Uuid::nil(),
            token: "nmr_secret".into(),
            image: "harness:1".into(),
            server_url: "http://hyperbeam:8080".into(),
            env: Default::default(),
        };
        let manifest = pod_manifest(&settings, &request);
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
        assert_eq!(spec.init_containers.unwrap()[0].image.as_deref(), Some("runner:1"));
    }
}
