//! Build a DataFusion `CatalogProvider` for one MySQL source, scoped
//! by the operator-configured `SourceAccessScope`.
//!
//! The connector ONLY exposes tables in scope (defence gate #1; see
//! memory: `eventhorizon-access-modes`). For unbounded scopes
//! (`WholeSchema` / `WholeDatabase`) the connector introspects
//! `INFORMATION_SCHEMA` to enumerate what exists; for bounded scopes
//! (`Table` / `Tables`) it honours the explicit allow-list and refuses
//! to start if any allow-listed table doesn't exist.
//!
//! The returned `CatalogProvider` holds one `MemorySchemaProvider` per
//! discovered MySQL schema, each containing one `MysqlTableProvider`
//! per in-scope table.

use std::collections::HashMap;
use std::sync::Arc;

use datafusion::catalog::{
    CatalogProvider, MemoryCatalogProvider, MemorySchemaProvider, SchemaProvider,
};
use eh_connector_api::{ConnectorError, ConnectorResult};
use eh_core::SourceAccessScope;
use sqlx::{MySql, Pool};
use tracing::{info, instrument};

use crate::introspection::{discover_tables, MysqlTable};
use crate::table_provider::MysqlTableProvider;

/// Build the catalog for one MySQL source, scoped to `scope`.
///
/// The result is what the `Connector::build_catalog` impl returns; the
/// gateway registers it with the always-on DataFusion `SessionContext`
/// under the source's operator-chosen name.
#[instrument(skip(pool, scope))]
pub(crate) async fn build_mysql_catalog(
    pool: &Pool<MySql>,
    scope: &SourceAccessScope,
) -> ConnectorResult<Arc<dyn CatalogProvider>> {
    let tables = discover_tables(pool, scope).await?;
    info!(
        target: "eh.connector.mysql.catalog",
        tables = tables.len(),
        "discovered tables for catalog"
    );

    if tables.is_empty() {
        return Err(ConnectorError::Backend(format!(
            "MySQL source produced an empty catalog for scope {scope:?}; \
             check the access scope, the table allow-list, and the eh_service \
             grants on INFORMATION_SCHEMA"
        )));
    }

    // Group tables by schema; each schema becomes a SchemaProvider.
    let mut by_schema: HashMap<String, Vec<MysqlTable>> = HashMap::new();
    for t in tables {
        by_schema.entry(t.schema_name.clone()).or_default().push(t);
    }

    let catalog = MemoryCatalogProvider::new();
    for (schema_name, schema_tables) in by_schema {
        let schema_provider = MemorySchemaProvider::new();
        for t in schema_tables {
            let table_name = t.table_name.clone();
            let provider = Arc::new(MysqlTableProvider::new(pool.clone(), t));
            schema_provider
                .register_table(table_name.clone(), provider)
                .map_err(|e| {
                    ConnectorError::Backend(format!(
                        "register_table {schema_name}.{table_name}: {e}"
                    ))
                })?;
        }
        catalog
            .register_schema(&schema_name, Arc::new(schema_provider))
            .map_err(|e| ConnectorError::Backend(format!("register_schema {schema_name}: {e}")))?;
    }

    Ok(Arc::new(catalog))
}
