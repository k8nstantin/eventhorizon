//! # eh-telemetry — Tier 1
//!
//! Phase 1 telemetry. Two pieces:
//!
//! * `init_tracing(filter)` — installs a JSON-formatted `tracing_subscriber`
//!   to stdout, filtered by either the supplied `EnvFilter` string or
//!   `RUST_LOG` if the supplied filter is empty.
//! * `install_prometheus()` — installs the global `metrics::Recorder` backed
//!   by Prometheus and returns a `PrometheusHandle` that the REST `/metrics`
//!   endpoint scrapes.
//!
//! Plus the shared metric-name constants every emitter uses (so name drift
//! is impossible across crates).
//!
//! Tier 2 (typed event bus + Sink trait + PG audit + Kafka) lands in
//! Phase 8 per architecture §10.0. Tier 1's tracing spans and metric
//! emissions are the same shape they will be in Tier 2 — the bus
//! subscribes to them rather than replacing them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use metrics_exporter_prometheus::PrometheusBuilder;
use thiserror::Error;
use tracing_subscriber::EnvFilter;

/// Re-export of the Prometheus handle the REST `/metrics` endpoint calls
/// `render()` on. Re-exported here so dependent crates (e.g., `eh-bin`,
/// `eh-edge-rest`) take only `eh-telemetry` as their dep surface for
/// telemetry types — the exporter crate is an implementation detail.
pub use metrics_exporter_prometheus::PrometheusHandle;

/// Stable metric names. Every emission across the workspace uses these
/// constants so renames are a single PR and dashboards survive.
pub mod metric_name {
    /// Per-intent end-to-end latency histogram (milliseconds).
    pub const INTENT_LATENCY_MS: &str = "eh_intent_latency_ms";
    /// Per-intent counter. Labels: `entity`, `action`, `outcome`.
    pub const INTENT_COUNT: &str = "eh_intent_count_total";
    /// Per-intent error counter. Labels: `entity`, `action`, `code`.
    pub const INTENT_ERROR_COUNT: &str = "eh_intent_error_count_total";
}

/// Stable span / metric label keys.
pub mod label {
    /// Logical entity name.
    pub const ENTITY: &str = "entity";
    /// Action — `read` | `append`.
    pub const ACTION: &str = "action";
    /// Outcome — `ok` | `denied` | `error` | etc.
    pub const OUTCOME: &str = "outcome";
    /// Error code from `eh_protocol::ErrorCode`.
    pub const CODE: &str = "code";
}

/// Errors `eh-telemetry` can raise during init. After init succeeds, no
/// error path remains; emission is best-effort.
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// The tracing subscriber could not be installed (likely because one
    /// was already installed in the same process). Surface but tolerate
    /// — re-init in a test runner is benign.
    #[error("tracing subscriber init failed: {0}")]
    TracingInit(String),

    /// The Prometheus recorder could not be installed.
    #[error("prometheus recorder install failed: {0}")]
    PrometheusInit(String),

    /// `RUST_LOG`-like filter string was invalid.
    #[error("invalid log filter {0:?}: {1}")]
    InvalidFilter(String, String),
}

/// Initialise the global tracing subscriber with JSON-formatted output to
/// stdout.
///
/// `filter` precedence:
/// 1. If non-empty, parsed as an `EnvFilter` directive.
/// 2. Otherwise, falls back to `RUST_LOG` env var.
/// 3. If both are absent, defaults to `info,eh=debug`.
///
/// Returns `Ok(())` on success. Returns `Err(TracingInit)` if another
/// subscriber is already installed (test runners may already have one).
pub fn init_tracing(filter: &str) -> Result<(), TelemetryError> {
    let env_filter = if filter.is_empty() {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,eh=debug"))
    } else {
        EnvFilter::try_new(filter)
            .map_err(|e| TelemetryError::InvalidFilter(filter.to_string(), e.to_string()))?
    };

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_current_span(true)
        .with_span_list(false)
        .try_init()
        .map_err(|e| TelemetryError::TracingInit(e.to_string()))
}

/// Install the global Prometheus recorder and return a handle.
///
/// The returned `PrometheusHandle` is what the REST `/metrics` endpoint
/// calls `render()` on to produce the scrape response.
pub fn install_prometheus() -> Result<PrometheusHandle, TelemetryError> {
    PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| TelemetryError::PrometheusInit(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_name_constants_are_stable() {
        assert_eq!(metric_name::INTENT_LATENCY_MS, "eh_intent_latency_ms");
        assert_eq!(metric_name::INTENT_COUNT, "eh_intent_count_total");
        assert_eq!(
            metric_name::INTENT_ERROR_COUNT,
            "eh_intent_error_count_total"
        );
    }

    #[test]
    fn label_constants_are_stable() {
        assert_eq!(label::ENTITY, "entity");
        assert_eq!(label::ACTION, "action");
        assert_eq!(label::OUTCOME, "outcome");
        assert_eq!(label::CODE, "code");
    }
}
