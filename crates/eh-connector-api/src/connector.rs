//! The `Connector` trait — the contract every backend implements.
//!
//! ## Direction of travel: connector becomes DUMB
//!
//! EventHorizon's steady-state architecture (decided 2026-05-21) has
//! DataFusion as the always-on agent <-> database firewall. The
//! connector's job shrinks to: declare its kind + capabilities, expose
//! a `TableProvider` for each binding. DataFusion does all parsing,
//! validation, planning, optimization, federation, SQL generation —
//! and calls into the `TableProvider` to actually fetch / write rows.
//!
//! The `execute_read` / `execute_append` methods are TRANSITIONAL —
//! they exist for Phase 1.7 (Intent-only FVP). PR 1.8.5 reroutes
//! dispatch through DataFusion; PR 1.8.7 deletes those methods.
//! Connectors should NOT add new logic to them; new work goes into
//! the `TableProvider` impl returned by `as_table_provider`.
//!
//! Whether a given binding exposes read / append is controlled by the
//! YAML binding's `supported_actions` parameter (not by the connector
//! itself). The connector declares what it is *capable* of via
//! `capabilities()`; the binding's `supported_actions` further narrows
//! what reaches the connector at runtime.

use std::sync::Arc;

use async_trait::async_trait;
use datafusion::catalog::CatalogProvider;
use eh_core::{Artifact, CallerContext, Entity, EntityBinding, Intent, SourceAccessScope};

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

    /// Build a DataFusion `CatalogProvider` whose visible
    /// schemas/tables match the operator-configured `SourceAccessScope`.
    ///
    /// **This is the steady-state data path** — every connector
    /// implements it; the gateway registers the resulting catalog with
    /// the always-on DataFusion `SessionContext` under the source's
    /// name. DataFusion then drives reads (and writes when allowed)
    /// through the standard `TableProvider::scan` / `insert_into` API.
    ///
    /// ## Defence gate #1
    ///
    /// The connector MUST NOT expose any table outside `scope`, even
    /// if engine grants would allow it. Out-of-scope tables MUST NOT
    /// appear in the returned catalog. This is the first of the three
    /// independent gates between the agent and the database (the
    /// others being binding-level access for `sql_passthrough` and
    /// engine grants).
    ///
    /// ## Scope semantics
    ///
    /// * `SourceAccessScope::Table { schema, table }` — register one
    ///   `SchemaProvider` containing one `TableProvider`.
    /// * `SourceAccessScope::Tables { schema, tables }` — register one
    ///   `SchemaProvider` containing the allow-listed tables. Refuse
    ///   to start if any allow-listed table does not exist.
    /// * `SourceAccessScope::WholeSchema { schema }` — introspect the
    ///   backend (`information_schema` for SQL backends; equivalent
    ///   for others) and expose every table the connector can see in
    ///   that schema.
    /// * `SourceAccessScope::WholeDatabase { databases }` — introspect
    ///   each named database; expose one `SchemaProvider` per schema
    ///   found.
    ///
    /// ## Phase 1.8.3 status
    ///
    /// The trait surface is in place. `MysqlConnector` returns a
    /// typed `Backend("build_catalog not yet implemented in 1.8.3")`
    /// error; the real introspection + `TableProvider` impl lands in
    /// 1.8.4. New connector authors implement this method directly,
    /// not the transitional `execute_*` methods above.
    async fn build_catalog(
        &self,
        scope: &SourceAccessScope,
    ) -> ConnectorResult<Arc<dyn CatalogProvider>>;
}
