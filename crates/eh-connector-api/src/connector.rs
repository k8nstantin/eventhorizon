//! The `Connector` trait — the contract every backend implements.
//!
//! Two execution methods only: `execute_read` (SELECT-shaped) and
//! `execute_append` (INSERT-shaped). There is no UPDATE / DELETE / DDL
//! method, period. State changes are new rows under SCD2; destructive
//! operations belong to the operator under their admin role, not to the
//! application code path.
//!
//! Whether a given binding exposes read / append is controlled by the
//! YAML binding's `supported_actions` parameter (not by the connector
//! itself). The connector declares what it is *capable* of via
//! `capabilities()`; the binding's `supported_actions` further narrows
//! what reaches the connector at runtime.

use async_trait::async_trait;
use eh_core::{Artifact, CallerContext, Entity, EntityBinding, Intent};

use crate::caps::ConnectorCaps;
use crate::errors::ConnectorResult;
use crate::outcome::AppendOutcome;

/// What every backend implements.
///
/// Implementations are `Send + Sync + 'static` so they can be shared across
/// tokio worker threads in an `Arc`. Each implementor owns its own pool;
/// `connect` is called once at gateway startup per registered source.
#[async_trait]
pub trait Connector: Send + Sync + 'static {
    /// Stable kind identifier matching `eh_control.sources.kind` (e.g.,
    /// `"mysql"`, `"postgres"`, `"iceberg"`). Must be a lowercase ASCII
    /// identifier without dots or whitespace.
    fn kind(&self) -> &'static str;

    /// Declares what this connector implementation can do. Honest reporting
    /// is mandatory — the router and the conformance suite both trust it.
    fn capabilities(&self) -> ConnectorCaps;

    /// Liveness ping. Cheap; runs on a background timer in production.
    async fn health(&self) -> ConnectorResult<()>;

    /// Execute a `Read` intent against the binding's physical table.
    ///
    /// Implementations MUST:
    /// - Use parameterised queries (never string-concatenated user input).
    /// - Project the columns the intent requests (or all entity fields if
    ///   the intent omits `fields`), mapped through `binding.field_map`.
    /// - Apply the intent's `filter` as a parameterised WHERE clause.
    /// - Return rows keyed by **logical** field name in the artifact.
    async fn execute_read(
        &self,
        binding: &EntityBinding,
        entity: &Entity,
        intent: &Intent,
        ctx: &CallerContext,
    ) -> ConnectorResult<Artifact>;

    /// Execute an `Append` intent against the binding's physical table.
    ///
    /// Implementations MUST:
    /// - Emit INSERT only. Upserts and merges are forbidden (zero-trust §10).
    /// - Use parameterised queries.
    /// - Take field values from `intent.payload` keyed by logical field
    ///   name, mapped through `binding.field_map`.
    /// - Let the DB defaults handle the SCD2 triad (`valid_from`,
    ///   `valid_to`, `is_current`) — these MUST NOT be set explicitly.
    /// - Return `AppendOutcome` with the row count.
    async fn execute_append(
        &self,
        binding: &EntityBinding,
        entity: &Entity,
        intent: &Intent,
        ctx: &CallerContext,
    ) -> ConnectorResult<AppendOutcome>;
}
