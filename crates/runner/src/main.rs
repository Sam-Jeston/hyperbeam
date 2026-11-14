mod acp;
mod bridge;
mod conn;
#[cfg(unix)]
mod seed;
mod session;
mod workspace;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hyperbeam-runner", version, about = "Hyperbeam in-sandbox runner")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Control plane base URL.
    #[arg(long, env = "HYPERBEAM_SERVER_URL", global = true)]
    server_url: Option<String>,

    /// Per-session credential issued by the control plane.
    #[arg(long, env = "HYPERBEAM_RUN_TOKEN", global = true, hide_env_values = true)]
    run_token: Option<String>,

    /// Directory repositories are checked out into.
    #[arg(long, env = "HYPERBEAM_WORKSPACE", default_value = "/workspace", global = true)]
    workspace: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to the control plane and run one session (the default).
    Run,
    /// Copy this binary to DEST (used by the Kubernetes init container).
    Install { dest: PathBuf },
    /// Copy what an image keeps in FROM into the volume at TO (used by the
    /// Kubernetes init containers).
    #[cfg(unix)]
    Seed {
        from: PathBuf,
        to: PathBuf,
        /// Leave it all out, and a note behind, if FROM holds more than this.
        #[arg(long)]
        max_bytes: Option<u64>,
    },
    /// Bridge a stdio MCP client to the Hyperbeam tool gateway.
    McpBridge {
        #[arg(long, env = "HYPERBEAM_MCP_URL")]
        mcp_url: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // stdout belongs to the protocol in the agent and bridge modes.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match cli.command.unwrap_or(Command::Run) {
        Command::Install { dest } => {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(std::env::current_exe()?, &dest)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(())
        }
        #[cfg(unix)]
        Command::Seed { from, to, max_bytes } => {
            match seed::seed(&from, &to, max_bytes)? {
                seed::Outcome::Nothing => {}
                seed::Outcome::Copied { files, skipped } => {
                    tracing::info!("copied {files} files from {} ({skipped} could not be read)", from.display())
                }
                seed::Outcome::TooLarge { bytes } => tracing::warn!("{} holds {bytes} bytes, too much to carry over", from.display()),
            }
            Ok(())
        }
        Command::McpBridge { mcp_url } => {
            let token = cli.run_token.ok_or_else(|| anyhow::anyhow!("HYPERBEAM_RUN_TOKEN is required"))?;
            bridge::run(mcp_url, token).await
        }
        Command::Run => {
            let cfg = session::Config {
                server_url: cli.server_url.ok_or_else(|| anyhow::anyhow!("HYPERBEAM_SERVER_URL is required"))?,
                token: cli.run_token.ok_or_else(|| anyhow::anyhow!("HYPERBEAM_RUN_TOKEN is required"))?,
                workspace: cli.workspace,
            };
            session::run(cfg).await
        }
    }
}
