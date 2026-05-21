//! Typed errors the engine surfaces. Translated to wire `ErrorResponse`
//! by `eh-edge-rest::dispatch`.

use thiserror::Error;

/// Result alias.
pub type EngineResult<T> = Result<T, EngineError>;

/// Errors the DataFusion engine can produce. Each variant maps cleanly to
/// a wire `ErrorCode`; the gateway never leaks raw DataFusion errors to
/// the agent.
#[derive(Debug, Error)]
pub enum EngineError {
    /// SQL failed to parse. Surface the position so the agent (or copilot)
    /// can point at the offending token.
    #[error("SQL parse error: {0}")]
    Parse(String),

    /// Statement class is on the operator's denylist (DDL, DELETE, etc.).
    /// The statement parsed cleanly but is refused at the policy gate
    /// BEFORE the plan is built.
    #[error("statement {class:?} is denied by operator policy")]
    StatementDenied {
        /// Which class of statement was refused.
        class: crate::config::StatementClass,
    },

    /// Statement matched one of the operator's deny-regex patterns.
    #[error("statement rejected by deny-regex pattern #{index}: {pattern}")]
    DenyRegexMatched {
        /// Zero-based index into `EngineConfig.policy.deny_regex`.
        index: usize,
        /// The regex that matched (logged for operator diagnosis; never
        /// echoed back to the agent in the wire response).
        pattern: String,
    },

    /// Logical plan referenced a table the binding's `access` scope does
    /// not allow.
    #[error("table {table:?} is outside the binding's access scope")]
    TableOutsideAccessScope {
        /// Fully qualified table name as it appeared in the plan.
        table: String,
    },

    /// DataFusion planning failed (catalog miss, type error, etc.).
    /// Implementation detail of the engine; the wire surface gets a
    /// generic `InvalidIntent` translation.
    #[error("plan build failed: {0}")]
    PlanBuild(String),

    /// Execution failed mid-stream (timeout, OOM, connector error
    /// propagated through `TableProvider`).
    #[error("execution failed: {0}")]
    Execution(String),

    /// The session context did not have a table provider registered for
    /// the requested source name — operator misconfiguration.
    #[error("no table provider registered for source {0:?}")]
    UnknownSource(String),
}
