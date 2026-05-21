//! The `Engine` — a thin wrapper around a DataFusion `SessionContext`,
//! the operator's `EngineConfig`, and the connector catalog.
//!
//! This scaffold lands the surface; the Intent → `LogicalPlan` compiler,
//! the parse-time policy gate, the per-source `TableProvider`
//! registration, and the execution + row-shaping loop arrive in
//! subsequent commits of this PR (in order):
//!
//!   * 1.8.2 — `Connector::as_table_provider` is added; `MysqlConnector`
//!     gains a `TableProvider` impl.
//!   * 1.8.3 — `Engine::register_connectors` registers each connector's
//!     `TableProvider` under its source name.
//!   * 1.8.4 — `Engine::compile_intent` builds a `LogicalPlan` from
//!     `(Intent, EntityBinding, Entity)`.
//!   * 1.8.5 — `Engine::execute` runs a plan, applies LIMIT caps from
//!     `LimitsConfig`, and shapes the `RecordBatch` stream into
//!     `eh_core::Artifact` rows keyed by logical field name.
//!   * 1.8.6 — parse-time AST walker enforces `PolicyConfig`.

use datafusion::execution::context::SessionContext;
use tracing::info;

use crate::config::EngineConfig;

/// The always-on DataFusion engine for one gateway process.
pub struct Engine {
    ctx: SessionContext,
    config: EngineConfig,
}

impl Engine {
    /// Build a new engine with the operator-supplied config. The DF
    /// `SessionContext` is created with defaults; per-engine optimizer
    /// and resource tweaks driven by `EngineConfig` are wired in 1.8.5.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        info!(
            target: "eh.engine.df",
            deny_statements = ?config.policy.deny_statements,
            max_query_seconds = config.limits.max_query_seconds,
            max_rows_returned = config.limits.max_rows_returned,
            "DataFusion engine initialised",
        );
        Self {
            ctx: SessionContext::new(),
            config,
        }
    }

    /// Read-only access to the underlying DF context. Exposed so
    /// `eh-edge-rest` (and tests) can submit SQL through the same
    /// session that holds the registered table providers. The engine
    /// owns the lifecycle; do not call `SessionContext::register_*`
    /// from outside.
    #[must_use]
    pub fn session(&self) -> &SessionContext {
        &self.ctx
    }

    /// Operator policy currently in force.
    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_is_constructible_with_defaults() {
        let e = Engine::new(EngineConfig::default());
        // Sanity: session context is alive; policy is the safe default.
        assert!(!e.config().policy.deny_statements.is_empty());
        let _ = e.session();
    }
}
