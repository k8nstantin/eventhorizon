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

use std::net::{IpAddr, SocketAddr};

use anyhow::Context;
use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Tier-1 telemetry: JSON tracing + Prometheus recorder. Log filter
    // honours `RUST_LOG` (and falls back to a sensible default inside
    // eh_telemetry); no hardcoded filter string here.
    if let Err(e) = eh_telemetry::init_tracing("") {
        eprintln!("warning: tracing init: {e}");
    }

    let port: u16 = std::env::var("EH_PORT")
        .ok()
        .as_deref()
        .map(str::parse)
        .transpose()
        .context("EH_PORT must be a valid u16 if set")?
        .unwrap_or(8080);

    // Bind address is operator-controlled via `EH_BIND_ADDR`. Default is
    // 0.0.0.0 because the binary is normally containerised; operators
    // running on a host pin to a specific interface by setting the env var.
    let bind_ip: IpAddr = std::env::var("EH_BIND_ADDR")
        .ok()
        .as_deref()
        .unwrap_or("0.0.0.0")
        .parse()
        .context("EH_BIND_ADDR must be a valid IP address")?;

    let addr: SocketAddr = SocketAddr::new(bind_ip, port);

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
