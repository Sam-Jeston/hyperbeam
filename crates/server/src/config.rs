use clap::{Args, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DriverKind {
    /// Run each agent as a local process. No isolation: development only.
    Process,
    /// Run each agent in its own pod.
    Kubernetes,
}

#[derive(Debug, Clone, Args)]
pub struct Config {
    #[arg(long, env = "HYPERBEAM_DATABASE_URL", hide_env_values = true)]
    pub database_url: String,

    /// 32 random bytes, base64. Encrypts stored credentials. See `gen-key`.
    #[arg(long, env = "HYPERBEAM_MASTER_KEY", hide_env_values = true)]
    pub master_key: String,

    #[arg(long, env = "HYPERBEAM_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: String,

    /// Address of the egress allowlist proxy. Empty disables it.
    #[arg(long, env = "HYPERBEAM_EGRESS_LISTEN", default_value = "")]
    pub egress_listen: String,

    /// URL people use to reach the UI (for invite links).
    #[arg(long, env = "HYPERBEAM_PUBLIC_URL", default_value = "http://localhost:8080")]
    pub public_url: String,

    /// URL sandboxes use to reach this server.
    #[arg(long, env = "HYPERBEAM_INTERNAL_URL", default_value = "http://127.0.0.1:8080")]
    pub internal_url: String,

    /// `host:port` sandboxes use to reach the egress proxy. When set, agents
    /// are started with HTTPS_PROXY pointing at it.
    #[arg(long, env = "HYPERBEAM_EGRESS_ADDR", default_value = "")]
    pub egress_addr: String,

    /// Hosts every sandbox may reach through the egress proxy, in addition to
    /// those its harness profile lists. `*.example.com` matches subdomains.
    #[arg(
        long,
        env = "HYPERBEAM_EGRESS_ALLOW",
        value_delimiter = ',',
        default_value = "registry.npmjs.org,pypi.org,files.pythonhosted.org,crates.io,static.crates.io,index.crates.io,proxy.golang.org,sum.golang.org"
    )]
    pub egress_allow: Vec<String>,

    #[arg(long, env = "HYPERBEAM_SANDBOX_DRIVER", value_enum, default_value = "process")]
    pub sandbox_driver: DriverKind,

    /// Process driver: path to the runner binary. Defaults to a
    /// `hyperbeam-runner` next to this executable.
    #[arg(long, env = "HYPERBEAM_RUNNER_BIN")]
    pub runner_bin: Option<PathBuf>,

    /// Process driver: where workspaces are created.
    #[arg(long, env = "HYPERBEAM_WORKSPACE_ROOT")]
    pub workspace_root: Option<PathBuf>,

    #[arg(long, env = "HYPERBEAM_K8S_NAMESPACE", default_value = "hyperbeam-agents")]
    pub k8s_namespace: String,

    /// Kubernetes driver: image that carries the runner binary.
    #[arg(long, env = "HYPERBEAM_RUNNER_IMAGE", default_value = "ghcr.io/hyperbeam/runner:latest")]
    pub runner_image: String,

    #[arg(long, env = "HYPERBEAM_K8S_IMAGE_PULL_POLICY", default_value = "IfNotPresent")]
    pub k8s_image_pull_policy: String,

    #[arg(long, env = "HYPERBEAM_K8S_CPU_LIMIT", default_value = "2")]
    pub k8s_cpu_limit: String,

    #[arg(long, env = "HYPERBEAM_K8S_MEMORY_LIMIT", default_value = "4Gi")]
    pub k8s_memory_limit: String,

    #[arg(long, env = "HYPERBEAM_K8S_WORKSPACE_SIZE", default_value = "10Gi")]
    pub k8s_workspace_size: String,

    /// An idle session's sandbox is torn down after this long. The session
    /// can still be resumed.
    #[arg(long, env = "HYPERBEAM_IDLE_TIMEOUT_SECS", default_value_t = 900)]
    pub idle_timeout_secs: u64,

    /// A sandbox that has not connected within this long is given up on.
    #[arg(long, env = "HYPERBEAM_START_TIMEOUT_SECS", default_value_t = 600)]
    pub start_timeout_secs: u64,

    /// Most sandboxes alive at once.
    #[arg(long, env = "HYPERBEAM_MAX_SESSIONS", default_value_t = 20)]
    pub max_sessions: usize,

    #[arg(long, env = "HYPERBEAM_GITHUB_API_URL", default_value = "https://api.github.com")]
    pub github_api_url: String,

    #[arg(long, env = "HYPERBEAM_GITHUB_GIT_URL", default_value = "https://github.com")]
    pub github_git_url: String,

    #[arg(long, env = "HYPERBEAM_LINEAR_API_URL", default_value = "https://api.linear.app/graphql")]
    pub linear_api_url: String,

    #[arg(long, env = "HYPERBEAM_NOTION_API_URL", default_value = "https://api.notion.com/v1")]
    pub notion_api_url: String,
}
