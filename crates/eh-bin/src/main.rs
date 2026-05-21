//! # eh-bin
//!
//! EventHorizon gateway binary. Wires together the kernel modules + selected
//! edges + selected connectors per Cargo features.
//!
//! ## Phase 0 status
//! This binary currently exposes only `GET /healthz`. It is the deployable
//! artifact that proves the workspace + Docker + docker-compose topology
//! works. Real intent handling arrives in Phase 1.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::net::SocketAddr;

use anyhow::Context;
use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let port: u16 = std::env::var("EH_PORT")
        .ok()
        .as_deref()
        .map(str::parse)
        .transpose()
        .context("EH_PORT must be a valid u16 if set")?
        .unwrap_or(8080);

    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], port));

    let app: Router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    info!(
        target: "eh.startup",
        addr = %addr,
        version = env!("CARGO_PKG_VERSION"),
        "EventHorizon listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    info!(target: "eh.shutdown", "EventHorizon stopped");
    Ok(())
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,eh=debug"));

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_current_span(true)
        .with_span_list(false)
        .init();
}

#[derive(Serialize)]
struct HealthBody {
    status: &'static str,
    version: &'static str,
}

async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthBody {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
        }),
    )
}

async fn readyz() -> impl IntoResponse {
    // Phase 0: nothing downstream to probe yet. /readyz mirrors /healthz.
    // Real readiness (control DB ping, source pings) arrives in Phase 1.
    healthz().await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => warn!(target: "eh.shutdown", "received Ctrl+C, draining…"),
        () = terminate => warn!(target: "eh.shutdown", "received SIGTERM, draining…"),
    }
}
