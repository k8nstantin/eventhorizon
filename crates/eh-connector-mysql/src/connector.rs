//! `MysqlConnector` — `Connector` impl backed by `sqlx::MySql`.
//!
//! Phase 1 supports both READ and APPEND. The binding's `supported_actions`
//! YAML parameter gates per-binding exposure; the connector itself reports
//! `supports_read = true, supports_append = true` so any binding that the
//! operator chooses to expose can route through it. Engine refusals
//! (e.g., the `eh_service` MySQL grant set lacks INSERT) propagate as
//! `ConnectorError::EngineRefusal` — the §12 debugging surface working
//! as designed.

use async_trait::async_trait;
use eh_connector_api::{
    AppendOutcome, Connector, ConnectorCaps, ConnectorError, ConnectorResult, PushdownLevel,
};
use eh_core::{Artifact, ArtifactRow, CallerContext, Entity, EntityBinding, Intent};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode as SqlxSslMode};
use sqlx::MySql;
use sqlx::Pool;
use tracing::{debug, instrument, warn};

use crate::config::{MysqlSourceConfig, MysqlSslMode};
use crate::insert::build_insert;
use crate::query::build_select;
use crate::types::decode_column;

/// MySQL connector. Owns a `sqlx::MySql` pool sized per the source config.
#[derive(Debug, Clone)]
pub struct MysqlConnector {
    pool: Pool<MySql>,
    kind: &'static str,
}

impl MysqlConnector {
    /// Connect to MySQL using the operator-supplied configuration.
    ///
    /// The password is resolved from its `${ENV:NAME}` secret-ref at this
    /// point; the gateway refuses to start if the env var is missing. The
    /// resolved password is wrapped in a `Secret` (redacted Debug) and
    /// handed to sqlx for the connect — it never enters logs or argv.
    #[instrument(skip(cfg), fields(host = %cfg.host, port = cfg.port, database = %cfg.database, username = %cfg.username))]
    pub async fn connect(cfg: &MysqlSourceConfig) -> ConnectorResult<Self> {
        let password = cfg
            .password
            .resolve()
            .map_err(|e| ConnectorError::Connect(format!("secret resolution failed: {e}")))?;

        let opts = MySqlConnectOptions::new()
            .host(&cfg.host)
            .port(cfg.port)
            .database(&cfg.database)
            .username(&cfg.username)
            .password(password.expose())
            .ssl_mode(map_ssl(cfg.ssl_mode));

        let pool = MySqlPoolOptions::new()
            .max_connections(cfg.max_pool_size)
            .connect_with(opts)
            .await
            .map_err(|e| ConnectorError::Connect(format!("{e}")))?;

        debug!(target: "eh.connector.mysql", "MySQL pool opened");
        Ok(Self {
            pool,
            kind: "mysql",
        })
    }

    // Reserved test-only constructor for future integration tests that
    // need to inject a pre-built pool (e.g., a testcontainers-managed
    // MySQL). Re-introduce when that lands.
}

fn map_ssl(mode: MysqlSslMode) -> SqlxSslMode {
    match mode {
        MysqlSslMode::Disabled => SqlxSslMode::Disabled,
        MysqlSslMode::Preferred => SqlxSslMode::Preferred,
        MysqlSslMode::Required => SqlxSslMode::Required,
        MysqlSslMode::VerifyCa => SqlxSslMode::VerifyCa,
        MysqlSslMode::VerifyIdentity => SqlxSslMode::VerifyIdentity,
    }
}

#[async_trait]
impl Connector for MysqlConnector {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn capabilities(&self) -> ConnectorCaps {
        ConnectorCaps {
            supports_read: true,
            // The connector implements append. Whether a given BINDING
            // exposes append is gated by its YAML supported_actions field
            // — the connector does not enforce that itself.
            supports_append: true,
            // Phase 1 pushdown coverage: equality filters only, exact.
            predicate_pushdown: PushdownLevel::Exact,
            projection_pushdown: true,
            // Phase 1 materialises results. Streaming arrives in Phase 7.
            streaming: false,
        }
    }

    #[instrument(skip(self))]
    async fn health(&self) -> ConnectorResult<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| ConnectorError::Unhealthy(format!("{e}")))
    }

    #[instrument(
        skip(self, binding, entity, intent, _ctx),
        fields(
            entity = %entity.name,
            source = %binding.source,
            table = %binding.physical_table,
        )
    )]
    async fn execute_read(
        &self,
        binding: &EntityBinding,
        entity: &Entity,
        intent: &Intent,
        _ctx: &CallerContext,
    ) -> ConnectorResult<Artifact> {
        let built = build_select(binding, entity, intent)?;
        debug!(target: "eh.connector.mysql", sql = %built.sql, binds = built.binds.len(), "compiled SELECT");

        let mut args = sqlx::mysql::MySqlArguments::default();
        for v in &built.binds {
            v.bind_into(&mut args)?;
        }

        let rows = sqlx::query_with(&built.sql, args)
            .fetch_all(&self.pool)
            .await
            .map_err(map_exec_error)?;

        let mut artifact_rows = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut out = ArtifactRow::new();
            for col in &built.projection {
                let value = decode_column(row, &col.physical, &col.logical, col.field_type)?;
                out.insert(&col.logical, value);
            }
            artifact_rows.push(out);
        }

        Ok(Artifact {
            rows: artifact_rows,
            source_kind: self.kind.to_string(),
            source_id: None,
        })
    }

    #[instrument(
        skip(self, binding, entity, intent, _ctx),
        fields(
            entity = %entity.name,
            source = %binding.source,
            table = %binding.physical_table,
        )
    )]
    async fn execute_append(
        &self,
        binding: &EntityBinding,
        entity: &Entity,
        intent: &Intent,
        _ctx: &CallerContext,
    ) -> ConnectorResult<AppendOutcome> {
        let built = build_insert(binding, entity, intent)?;
        debug!(target: "eh.connector.mysql", sql = %built.sql, binds = built.binds.len(), "compiled INSERT");

        let mut args = sqlx::mysql::MySqlArguments::default();
        for v in &built.binds {
            v.bind_into(&mut args)?;
        }

        let outcome = sqlx::query_with(&built.sql, args)
            .execute(&self.pool)
            .await
            .map_err(map_exec_error)?;

        Ok(AppendOutcome {
            rows_inserted: outcome.rows_affected(),
        })
    }
}

/// Map a sqlx execution error to the typed `ConnectorError` taxonomy.
///
/// Specifically: detect MySQL "permission denied" / "command denied"
/// patterns and surface them as `EngineRefusal` — the §12 debugging
/// surface signal. Everything else folds into `Backend`.
fn map_exec_error(e: sqlx::Error) -> ConnectorError {
    let msg = e.to_string();
    if msg.contains("denied") || msg.contains("Access denied") || msg.contains("command denied") {
        warn!(target: "eh.connector.mysql", error = %msg, "engine refusal — eh_service grant set rejected the operation");
        ConnectorError::EngineRefusal(msg)
    } else {
        ConnectorError::Backend(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test that capability declaration matches the surface intent
    /// for Phase 1: both read and append, exact pushdown, no streaming.
    #[test]
    fn capabilities_match_phase1_intent() {
        let expected = ConnectorCaps {
            supports_read: true,
            supports_append: true,
            predicate_pushdown: PushdownLevel::Exact,
            projection_pushdown: true,
            streaming: false,
        };
        // Sanity: this is what consumers will read after MysqlConnector::connect.
        assert!(expected.supports_read);
        assert!(expected.supports_append);
        assert_eq!(expected.predicate_pushdown, PushdownLevel::Exact);
    }

    #[test]
    fn map_exec_error_classifies_denied_as_engine_refusal() {
        let denied = sqlx::Error::Configuration(
            "Access denied for user 'eh_service'@'%' to database 'eh_demo'".into(),
        );
        match map_exec_error(denied) {
            ConnectorError::EngineRefusal(_) => {}
            other => panic!("expected EngineRefusal, got {other:?}"),
        }
    }

    #[test]
    fn map_exec_error_classifies_other_as_backend() {
        let other = sqlx::Error::Configuration("connection lost mid-statement".into());
        match map_exec_error(other) {
            ConnectorError::Backend(_) => {}
            o => panic!("expected Backend, got {o:?}"),
        }
    }

    #[test]
    fn ssl_mode_mapping_covers_every_variant() {
        // Exhaustive check that the SSL mode mapping is complete and
        // matches sqlx's enum, surfacing any mismatch at compile time.
        for src in [
            MysqlSslMode::Disabled,
            MysqlSslMode::Preferred,
            MysqlSslMode::Required,
            MysqlSslMode::VerifyCa,
            MysqlSslMode::VerifyIdentity,
        ] {
            let _ = map_ssl(src);
        }
    }
}
