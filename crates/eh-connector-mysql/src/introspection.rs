//! Discover schemas, tables, and columns from the configured MySQL
//! instance.
//!
//! Used at `build_catalog` time: for each `SourceAccessScope` variant
//! we either honour the explicit allow-list (`Table` / `Tables`) or
//! query `INFORMATION_SCHEMA` to enumerate what exists
//! (`WholeSchema` / `WholeDatabase`). Every returned `MysqlTable`
//! carries its column list + a ready-made Arrow `Schema` so the
//! `CatalogProvider` can hand a fully-typed `TableProvider` to
//! DataFusion immediately.

use std::sync::Arc;

use datafusion::arrow::datatypes::Schema as ArrowSchema;
use eh_connector_api::{ConnectorError, ConnectorResult};
use eh_core::SourceAccessScope;
use sqlx::{mysql::MySqlRow, MySql, Pool, Row};
use tracing::{debug, instrument};

use crate::arrow_schema::MysqlColumn;

/// One discovered table + the Arrow schema the connector will hand
/// DataFusion. Built once per `build_catalog`.
#[derive(Debug, Clone)]
pub(crate) struct MysqlTable {
    /// MySQL schema (database) name.
    pub schema_name: String,
    /// Table name within that schema.
    pub table_name: String,
    /// Ordered column descriptors (positional).
    pub columns: Vec<MysqlColumn>,
    /// Pre-built Arrow schema matching `columns` in order.
    pub arrow_schema: Arc<ArrowSchema>,
}

/// Enumerate every in-scope table and fetch its column metadata.
///
/// Honours the operator's `SourceAccessScope`:
/// * `Table` / `Tables` — explicit allow-list; refuses to start if any
///   listed table does not exist or the operator's `eh_service` grant
///   cannot read its `INFORMATION_SCHEMA` row.
/// * `WholeSchema` / `WholeDatabase` — query `INFORMATION_SCHEMA.TABLES`
///   for `BASE TABLE`s only (views deliberately excluded from Phase 1).
#[instrument(skip(pool, scope), fields(scope_kind = scope_kind(scope)))]
pub(crate) async fn discover_tables(
    pool: &Pool<MySql>,
    scope: &SourceAccessScope,
) -> ConnectorResult<Vec<MysqlTable>> {
    let candidates: Vec<(String, String)> = match scope.explicit_tables() {
        Some(list) => list,
        None => list_tables_in_scope(pool, scope).await?,
    };

    let mut out = Vec::with_capacity(candidates.len());
    for (schema_name, table_name) in candidates {
        let columns = fetch_columns(pool, &schema_name, &table_name).await?;
        if columns.is_empty() {
            return Err(ConnectorError::Backend(format!(
                "table {schema_name}.{table_name} has no columns visible to eh_service \
                 (likely missing GRANTs or table does not exist)"
            )));
        }
        let mut arrow_fields = Vec::with_capacity(columns.len());
        for c in &columns {
            arrow_fields.push(c.arrow_field()?);
        }
        let arrow_schema = Arc::new(ArrowSchema::new(arrow_fields));
        debug!(
            target: "eh.connector.mysql.introspect",
            schema = %schema_name,
            table = %table_name,
            columns = columns.len(),
            "table discovered"
        );
        out.push(MysqlTable {
            schema_name,
            table_name,
            columns,
            arrow_schema,
        });
    }
    Ok(out)
}

async fn list_tables_in_scope(
    pool: &Pool<MySql>,
    scope: &SourceAccessScope,
) -> ConnectorResult<Vec<(String, String)>> {
    let schemas: Vec<String> = match scope {
        SourceAccessScope::WholeSchema { schema } => vec![schema.clone()],
        SourceAccessScope::WholeDatabase { databases } => databases.clone(),
        SourceAccessScope::Table { .. } | SourceAccessScope::Tables { .. } => {
            // explicit_tables() already returned Some — should not reach.
            return Ok(Vec::new());
        }
    };

    let mut out = Vec::new();
    for schema in schemas {
        let rows = sqlx::query(
            "SELECT table_schema, table_name \
             FROM information_schema.tables \
             WHERE table_schema = ? AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
        )
        .bind(&schema)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            ConnectorError::Backend(format!(
                "information_schema.tables read failed for schema {schema:?}: {e}"
            ))
        })?;

        for row in rows {
            let s: String = row
                .try_get("TABLE_SCHEMA")
                .or_else(|_| row.try_get("table_schema"))
                .map_err(|e| ConnectorError::Backend(format!("{e}")))?;
            let t: String = row
                .try_get("TABLE_NAME")
                .or_else(|_| row.try_get("table_name"))
                .map_err(|e| ConnectorError::Backend(format!("{e}")))?;
            out.push((s, t));
        }
    }
    Ok(out)
}

async fn fetch_columns(
    pool: &Pool<MySql>,
    schema_name: &str,
    table_name: &str,
) -> ConnectorResult<Vec<MysqlColumn>> {
    let rows = sqlx::query(
        "SELECT column_name, data_type, column_type, is_nullable, \
                numeric_precision, numeric_scale \
         FROM information_schema.columns \
         WHERE table_schema = ? AND table_name = ? \
         ORDER BY ordinal_position",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        ConnectorError::Backend(format!(
            "information_schema.columns read failed for {schema_name}.{table_name}: {e}"
        ))
    })?;

    let mut cols = Vec::with_capacity(rows.len());
    for row in &rows {
        cols.push(column_from_row(row)?);
    }
    Ok(cols)
}

fn column_from_row(row: &MySqlRow) -> ConnectorResult<MysqlColumn> {
    let name: String = row
        .try_get("COLUMN_NAME")
        .or_else(|_| row.try_get("column_name"))
        .map_err(|e| ConnectorError::Backend(format!("column_name read: {e}")))?;
    let data_type: String = row
        .try_get("DATA_TYPE")
        .or_else(|_| row.try_get("data_type"))
        .map(|s: String| s.to_lowercase())
        .map_err(|e| ConnectorError::Backend(format!("data_type read for {name}: {e}")))?;
    let column_type: String = row
        .try_get("COLUMN_TYPE")
        .or_else(|_| row.try_get("column_type"))
        .map(|s: String| s.to_lowercase())
        .map_err(|e| ConnectorError::Backend(format!("column_type read for {name}: {e}")))?;
    let is_nullable_str: String = row
        .try_get("IS_NULLABLE")
        .or_else(|_| row.try_get("is_nullable"))
        .map_err(|e| ConnectorError::Backend(format!("is_nullable read for {name}: {e}")))?;
    let nullable = is_nullable_str.eq_ignore_ascii_case("YES");

    let decimal_precision: Option<u8> = row
        .try_get::<Option<i64>, _>("NUMERIC_PRECISION")
        .or_else(|_| row.try_get::<Option<i64>, _>("numeric_precision"))
        .ok()
        .flatten()
        .and_then(|n| u8::try_from(n).ok());
    let decimal_scale: Option<i8> = row
        .try_get::<Option<i64>, _>("NUMERIC_SCALE")
        .or_else(|_| row.try_get::<Option<i64>, _>("numeric_scale"))
        .ok()
        .flatten()
        .and_then(|n| i8::try_from(n).ok());

    Ok(MysqlColumn {
        name,
        data_type,
        column_type,
        nullable,
        decimal_precision,
        decimal_scale,
    })
}

fn scope_kind(scope: &SourceAccessScope) -> &'static str {
    match scope {
        SourceAccessScope::Table { .. } => "table",
        SourceAccessScope::Tables { .. } => "tables",
        SourceAccessScope::WholeSchema { .. } => "whole_schema",
        SourceAccessScope::WholeDatabase { .. } => "whole_database",
    }
}
