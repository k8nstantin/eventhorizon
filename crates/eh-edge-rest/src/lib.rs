//! # eh-edge-rest
//!
//! REST edge for the gateway. Exposes three endpoints:
//!
//! * `POST /v1/intent` — accepts an `IntentEnvelope`, dispatches through
//!   the router → compiler validator → connector pipeline, returns a
//!   `ResponseEnvelope`.
//! * `GET  /healthz`  — process-alive probe (`200 OK`, JSON body).
//! * `GET  /metrics`  — Prometheus scrape endpoint backed by the global
//!   `PrometheusHandle` installed by `eh_telemetry::install_prometheus()`.
//!
//! Wiring is operator-driven: `eh-bin` constructs an `AppState` with the
//! loaded `ConfigCache` + `ConnectorRegistry` + a per-source map of
//! ready-to-use `Arc<dyn Connector>` instances, then mounts this crate's
//! `router(state)` into the axum server.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod dispatch;
mod state;

pub use dispatch::dispatch_intent;
pub use state::{AppState, ConnectorMap};

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use eh_protocol::IntentEnvelope;
use metrics_exporter_prometheus::PrometheusHandle;
use serde::Serialize;
use std::sync::Arc;

/// Construct the axum router with all REST endpoints mounted.
///
/// The `prometheus` handle is captured by the `/metrics` handler closure;
/// pass `None` to disable the endpoint (operator deployment choice — e.g.,
/// when scraping is configured via a sidecar process instead).
pub fn router(state: Arc<AppState>, prometheus: Option<PrometheusHandle>) -> Router {
    let mut r = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/intent", post(post_intent))
        .with_state(state);

    if let Some(handle) = prometheus {
        r = r.route(
            "/metrics",
            get(move || {
                let h = handle.clone();
                async move { h.render() }
            }),
        );
    }

    r
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
    // Phase 1: alive == ready (no upstream probes yet — those land with
    // Cedar / cost gating / OTel propagation in later phases).
    healthz().await
}

/// `POST /v1/intent` handler. Translates the wire envelope into the
/// dispatch call, then folds the result into a `ResponseEnvelope`.
async fn post_intent(
    State(state): State<Arc<AppState>>,
    Json(envelope): Json<IntentEnvelope>,
) -> impl IntoResponse {
    let response = dispatch::dispatch_intent(state.as_ref(), envelope).await;
    let status = if response.is_success() {
        StatusCode::OK
    } else {
        // Phase 1: every error returns 400. Phase 5+ refines (e.g., 403
        // for authz denials, 429 for rate limits) when we have those
        // signals. The structured body remains authoritative; the HTTP
        // code is a hint for clients that don't parse the body.
        StatusCode::BAD_REQUEST
    };
    (status, Json(response))
}
