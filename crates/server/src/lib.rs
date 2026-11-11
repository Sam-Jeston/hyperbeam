//! Hyperbeam control plane.

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod connections;
pub mod crypto;
pub mod egress;
pub mod error;
pub mod gateway;
pub mod gitproxy;
pub mod hub;
pub mod integrations;
pub mod probe;
pub mod sandbox;
pub mod sessions;
pub mod store;
pub mod web;

use anyhow::{Context, Result};
use app::App;
use axum::Router;
use axum::routing::{any, get, post};
use config::{Config, DriverKind};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

/// Connect to the database, migrate, and assemble the application.
pub async fn build(cfg: Config) -> Result<(App, Router)> {
    let secrets = crypto::SecretBox::from_base64(&cfg.master_key).context("HYPERBEAM_MASTER_KEY")?;
    // The database often starts alongside the server (same Helm release,
    // same compose file); wait for it rather than crash-looping.
    let options = PgPoolOptions::new().max_connections(20).acquire_timeout(std::time::Duration::from_secs(10));
    let started = std::time::Instant::now();
    let db = loop {
        match options.clone().connect(&cfg.database_url).await {
            Ok(db) => break db,
            Err(err) if started.elapsed() < std::time::Duration::from_secs(90) => {
                tracing::info!("waiting for Postgres: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(err) => return Err(err).context("could not connect to Postgres (HYPERBEAM_DATABASE_URL)"),
        }
    };
    sqlx::migrate!("./migrations").run(&db).await.context("database migration failed")?;
    store::seed_harnesses(&db).await?;

    let driver: Arc<dyn sandbox::SandboxDriver> = match cfg.sandbox_driver {
        DriverKind::Process => {
            tracing::warn!("using the process sandbox driver: agents run unisolated on this host. Development only.");
            Arc::new(sandbox::process::ProcessDriver::new(cfg.runner_bin.clone(), cfg.workspace_root.clone())?)
        }
        DriverKind::Kubernetes => Arc::new(sandbox::kubernetes::KubernetesDriver::new(&cfg).await?),
    };

    let app = App::new(cfg, db, secrets, driver);
    app.adopt_legacy_linear().await.context("moving the Linear integration to a connection")?;
    app.recover().await?;
    tokio::spawn(app.clone().reaper());

    let router = Router::new()
        .nest("/api/v1", api::router())
        .route("/runner/v1/connect", get(hub::connect))
        .route("/mcp", post(gateway::handle).get(gateway::no_stream))
        .route("/git/{*path}", any(gitproxy::handle))
        .route("/healthz", get(|| async { "ok" }))
        .fallback(web::serve)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(app.clone());
    Ok((app, router))
}
