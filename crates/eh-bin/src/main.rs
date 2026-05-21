//! # eh-bin
//!
//! EventHorizon gateway binary. Composes the kernel modules + selected
//! edges + selected connectors per Cargo features.
//!
//! ## Startup pipeline (Phase 1 FVP)
//!
//! 1. Tier-1 telemetry: JSON `tracing_subscriber` + Prometheus recorder
//!    (`eh-telemetry`). `RUST_LOG` filter is honoured by `init_tracing("")`.
//! 2. Operator env-var contract:
//!      * `EH_CONFIG`       — required, path to the single YAML config.
//!      * `EH_BIND_ADDR`    — optional, default `0.0.0.0`.
//!      * `EH_PORT`         — optional, default `8080`.
//!      * `EH_DEFAULT_TENANT_ID` — required, UUID; Phase 1 single-tenant
//!        gateway uses it for every `CallerContext.tenant_id`. Phase 6
//!        pulls this from the agent record in the control plane.
//! 3. Load + validate the YAML config (`eh-config`).
//! 4. Build a `ConnectorRegistry`; register each connector under its
//!    feature flag (zero-trust §15 — kernel only knows the registry, not
//!    any specific connector type).
//! 5. For each source in the compiled config, call `registry.build` to
//!    instantiate a connector. Missing kinds surface a typed error
//!    pointing at the corresponding Cargo feature flag.
//! 6. Mount `eh-edge-rest::router` with the `AppState` (ConfigCache +
//!    ConnectorMap + default tenant) and the Prometheus handle.
//! 7. Serve until SIGTERM / Ctrl+C with graceful shutdown.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use eh_config::{CompiledConfig, ConfigCache};
use eh_connector_api::ConnectorRegistry;
use eh_edge_rest::AppState;
use eh_telemetry::PrometheusHandle;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = eh_telemetry::init_tracing("") {
        eprintln!("warning: tracing init: {e}");
    }
    let prometheus = eh_telemetry::install_prometheus().context("install Prometheus recorder")?;

    let addr = bind_addr()?;
    let default_tenant = default_tenant_id()?;

    let compiled = eh_config::load_from_env().context("load EH_CONFIG")?;
    info!(
        target: "eh.startup",
        sources = compiled.sources.len(),
        entities = compiled.entities.len(),
        bindings = compiled.bindings_by_entity.values().map(Vec::len).sum::<usize>(),
        routes = compiled.routing.len(),
        "config loaded",
    );

    let registry = build_registry()?;
    info!(
        target: "eh.startup",
        kinds = ?registry.known_kinds(),
        "connector registry populated",
    );

    let connectors = build_connectors(&registry, &compiled).await?;
    info!(
        target: "eh.startup",
        connectors = connectors.len(),
        "connector instances built",
    );

    let state = Arc::new(AppState::new(
        ConfigCache::new(compiled),
        connectors,
        default_tenant,
    ));

    serve(addr, state, prometheus).await
}

fn bind_addr() -> Result<SocketAddr> {
    let port: u16 = std::env::var("EH_PORT")
        .ok()
        .as_deref()
        .map(str::parse)
        .transpose()
        .context("EH_PORT must be a valid u16 if set")?
        .unwrap_or(8080);

    let bind_ip: IpAddr = std::env::var("EH_BIND_ADDR")
        .ok()
        .as_deref()
        .unwrap_or("0.0.0.0")
        .parse()
        .context("EH_BIND_ADDR must be a valid IP address")?;

    Ok(SocketAddr::new(bind_ip, port))
}

fn default_tenant_id() -> Result<Uuid> {
    let raw = std::env::var("EH_DEFAULT_TENANT_ID")
        .context("EH_DEFAULT_TENANT_ID is required (Phase 1 single-tenant gateway)")?;
    Uuid::parse_str(&raw).context("EH_DEFAULT_TENANT_ID must be a valid UUID")
}

fn build_registry() -> Result<ConnectorRegistry> {
    let mut registry = ConnectorRegistry::new();

    #[cfg(feature = "connector-mysql")]
    eh_connector_mysql::register(&mut registry).context("register mysql connector factory")?;

    Ok(registry)
}

async fn build_connectors(
    registry: &ConnectorRegistry,
    compiled: &CompiledConfig,
) -> Result<HashMap<String, Arc<dyn eh_connector_api::Connector>>> {
    let mut connectors = HashMap::with_capacity(compiled.sources.len());
    for (name, source) in &compiled.sources {
        let instance = registry
            .build(name, &source.kind, source.raw.clone())
            .await
            .with_context(|| {
                format!(
                    "build connector for source {name:?} (kind {:?})",
                    source.kind
                )
            })?;
        connectors.insert(name.clone(), instance);
    }
    Ok(connectors)
}

async fn serve(addr: SocketAddr, state: Arc<AppState>, prometheus: PrometheusHandle) -> Result<()> {
    let app = eh_edge_rest::router(state, Some(prometheus));

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    info!(
        target: "eh.startup",
        addr = %addr,
        version = env!("CARGO_PKG_VERSION"),
        "EventHorizon listening",
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    info!(target: "eh.shutdown", "EventHorizon stopped");
    Ok(())
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
