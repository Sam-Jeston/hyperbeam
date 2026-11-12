use clap::{Parser, Subcommand};
use hyperbeam_server::config::{Config, RotateArgs};
use hyperbeam_server::{build, crypto, egress, masterkey};

#[derive(Parser)]
#[command(name = "hyperbeam-server", version, about = "Hyperbeam control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the control plane.
    Serve(Box<Config>),
    /// Print a new master key, for HYPERBEAM_MASTER_KEY or to store in AWS Secrets Manager.
    GenKey,
    /// Re-encrypt every stored credential under a new master key. Reads the
    /// current key the way `serve` does; restart the server afterwards.
    RotateMasterKey(Box<RotateArgs>),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=error".into()))
        .init();

    match Cli::parse().command {
        Command::GenKey => {
            println!("{}", crypto::SecretBox::generate_key());
            Ok(())
        }
        Command::RotateMasterKey(args) => {
            let outcome = masterkey::rotate_configured(&args).await?;
            println!("{outcome}");
            Ok(())
        }
        Command::Serve(cfg) => {
            let listen = cfg.listen.clone();
            let egress_listen = cfg.egress_listen.clone();
            let (app, router) = build(*cfg).await?;

            if !egress_listen.is_empty() {
                let listener = tokio::net::TcpListener::bind(&egress_listen).await?;
                tracing::info!("egress proxy listening on {egress_listen}");
                tokio::spawn(egress::run(app.clone(), listener));
            }
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            tracing::info!("hyperbeam listening on {listen} (sandbox driver: {})", app.driver.name());
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await?;
            Ok(())
        }
    }
}
