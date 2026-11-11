use clap::{Parser, Subcommand};
use noemune_server::config::Config;
use noemune_server::{build, crypto, egress};

#[derive(Parser)]
#[command(name = "noemune-server", version, about = "Noemune control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the control plane.
    Serve(Box<Config>),
    /// Print a new master key for NOEMUNE_MASTER_KEY.
    GenKey,
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
            tracing::info!("noemune listening on {listen} (sandbox driver: {})", app.driver.name());
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await?;
            Ok(())
        }
    }
}
