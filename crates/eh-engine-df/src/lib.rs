//! # eh-engine-df — the always-on DataFusion engine
//!
//! EventHorizon's agent ↔ database firewall. Every action the agent
//! issues (Intent Read, Intent Append, ExecuteSql) flows through ONE
//! `datafusion::SessionContext`. The connector is a `TableProvider` that
//! DataFusion calls down to; it never dispatches independently.
//!
//! ## What lives here
//!
//! * [`EngineConfig`] — the operator policy parsed from the YAML
//!   `datafusion:` section. Knobs for deny-statements, deny-regex,
//!   query limits, pushdown levels, schema exposure, optimizer hints.
//!   Everything is operator-tunable; nothing is hardcoded.
//! * [`Engine`] — wraps the `SessionContext`, holds the connector
//!   catalog, executes plans, enforces policy. (Implementation lands in
//!   subsequent commits of this PR.)
//! * [`EngineError`] — typed errors the gateway translates to the wire
//!   `ErrorResponse` taxonomy.
//!
//! ## Why DataFusion sits here at all
//!
//! Without DF in front, every connector reinvents allow-list parsing,
//! pushdown, statement filtering. With DF in front: one parser, one
//! planner, one place to attach Cedar policies, one place to deny DDL,
//! one place to expose the schema DAG. The operator tunes policy via
//! YAML; zero code change.
//!
//! Three operational levers DF unlocks (memory: eventhorizon-access-modes):
//! REWRITE (inject RLS / LIMIT / column strip), REROUTE (per-plan-node
//! source placement, failover), QUARANTINE (sandbox / hold / elevated
//! telemetry).
//!
//! Designed so the async copilot (Gemma, Phase 8/9) can subscribe to
//! structured plan events without coupling the kernel to any LLM.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod engine;
mod errors;

pub use config::{
    EngineConfig, LimitsConfig, OptimizerConfig, PolicyConfig, PushdownConfig, PushdownPredicate,
    SchemaExposureConfig, StatementClass,
};
pub use engine::Engine;
pub use errors::{EngineError, EngineResult};
