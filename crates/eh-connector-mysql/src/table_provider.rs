//! `MysqlTableProvider` — DataFusion's view of one MySQL table.
//!
//! DataFusion drives the data path through this surface:
//!
//! 1. DF asks for the table's `schema()` (cached Arrow `SchemaRef`).
//! 2. DF calls `scan()` with the projection it wants, optional filters
//!    (which we declare as `Inexact` — DF will post-filter), and an
//!    optional `LIMIT`. We return a `MysqlScanExec` `ExecutionPlan`.
//! 3. DF calls `execute()` on the plan, which fires ONE parameterised
//!    `SELECT` against the `sqlx::MySql` pool and yields the rows as
//!    a single `RecordBatch` stream.
//!
//! Defence-in-depth notes (zero-trust §15):
//! - Column / table identifiers are wrapped in `safe_ident` before
//!   concatenation. No string-built SQL slips through.
//! - The SELECT is built from the OPERATOR-CONFIGURED schema/table
//!   names, NEVER from anything the agent supplied. The agent's
//!   filters (if any) are translated by DataFusion into `Expr`s; the
//!   connector does NOT splice them into the SQL in 1.8.4 (DF
//!   post-filters). Real predicate pushdown lands in 1.8.5 with a
//!   safe Expr-to-SQL translator.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::{Schema as ArrowSchema, SchemaRef};
use datafusion::catalog::Session;
use datafusion::datasource::TableProvider;
use datafusion::error::{DataFusionError, Result as DfResult};
use datafusion::execution::TaskContext;
use datafusion::logical_expr::{Expr, TableProviderFilterPushDown, TableType};
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, PlanProperties, SendableRecordBatchStream,
};
use sqlx::{MySql, Pool};
use tracing::{debug, instrument};

use crate::arrow_schema::{append_value, ArrayBuilderHandle};
use crate::ident::SafeIdent;
use crate::introspection::MysqlTable;

/// One DataFusion `TableProvider` per in-scope MySQL table.
#[derive(Debug)]
pub(crate) struct MysqlTableProvider {
    pool: Pool<MySql>,
    table: MysqlTable,
}

impl MysqlTableProvider {
    pub fn new(pool: Pool<MySql>, table: MysqlTable) -> Self {
        Self { pool, table }
    }
}

#[async_trait]
impl TableProvider for MysqlTableProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        self.table.arrow_schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> DfResult<Vec<TableProviderFilterPushDown>> {
        // Phase 1.8.4: the connector does NOT translate filters into
        // SQL WHERE clauses yet. DF keeps them and post-filters
        // in-process. Returning Inexact tells DF "I may or may not
        // honour these; post-filter to be sure."
        Ok(filters
            .iter()
            .map(|_| TableProviderFilterPushDown::Inexact)
            .collect())
    }

    #[instrument(
        skip(self, _state, projection, _filters, limit),
        fields(
            schema = %self.table.schema_name,
            table = %self.table.table_name,
            limit = ?limit,
        )
    )]
    async fn scan(
        &self,
        _state: &dyn Session,
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        let projected_schema: SchemaRef = match projection {
            Some(cols) => Arc::new(
                self.table
                    .arrow_schema
                    .project(cols)
                    .map_err(|e| DataFusionError::External(Box::new(e)))?,
            ),
            None => self.table.arrow_schema.clone(),
        };

        let projected_columns: Vec<crate::arrow_schema::MysqlColumn> = match projection {
            Some(cols) => cols
                .iter()
                .map(|i| self.table.columns[*i].clone())
                .collect(),
            None => self.table.columns.clone(),
        };

        let sql = build_select_sql(
            &self.table.schema_name,
            &self.table.table_name,
            &projected_columns,
            limit,
        )?;

        debug!(
            target: "eh.connector.mysql.scan",
            sql = %sql,
            rows_limit = ?limit,
            cols = projected_columns.len(),
            "compiled MySQL scan"
        );

        Ok(Arc::new(MysqlScanExec::new(
            self.pool.clone(),
            sql,
            projected_columns,
            projected_schema,
        )))
    }
}

fn build_select_sql(
    schema_name: &str,
    table_name: &str,
    cols: &[crate::arrow_schema::MysqlColumn],
    limit: Option<usize>,
) -> DfResult<String> {
    if cols.is_empty() {
        return Err(DataFusionError::Internal(
            "MysqlTableProvider::scan with zero projected columns".into(),
        ));
    }
    let schema_ident =
        SafeIdent::single(schema_name).map_err(|e| DataFusionError::External(Box::new(e)))?;
    let table_ident =
        SafeIdent::single(table_name).map_err(|e| DataFusionError::External(Box::new(e)))?;
    let col_idents: Vec<String> = cols
        .iter()
        .map(|c| {
            SafeIdent::single(&c.name)
                .map(|s| s.as_str().to_string())
                .map_err(|e| DataFusionError::External(Box::new(e)))
        })
        .collect::<DfResult<Vec<_>>>()?;
    let col_list = col_idents.join(", ");
    let mut sql = format!(
        "SELECT {col_list} FROM {}.{}",
        schema_ident.as_str(),
        table_ident.as_str()
    );
    if let Some(n) = limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    Ok(sql)
}

/// The `ExecutionPlan` `scan()` returns. Holds the pre-built SQL +
/// projected schema + pool handle; `execute()` runs the query and
/// streams the result back.
pub(crate) struct MysqlScanExec {
    pool: Pool<MySql>,
    sql: String,
    columns: Vec<crate::arrow_schema::MysqlColumn>,
    schema: SchemaRef,
    properties: Arc<PlanProperties>,
}

impl MysqlScanExec {
    fn new(
        pool: Pool<MySql>,
        sql: String,
        columns: Vec<crate::arrow_schema::MysqlColumn>,
        schema: SchemaRef,
    ) -> Self {
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema.clone()),
            datafusion::physical_expr::Partitioning::UnknownPartitioning(1),
            EmissionType::Final,
            Boundedness::Bounded,
        ));
        Self {
            pool,
            sql,
            columns,
            schema,
            properties,
        }
    }
}

impl fmt::Debug for MysqlScanExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MysqlScanExec")
            .field("sql", &self.sql)
            .field("columns", &self.columns.len())
            .finish()
    }
}

impl DisplayAs for MysqlScanExec {
    fn fmt_as(&self, _t: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MysqlScanExec: sql={}", self.sql)
    }
}

impl ExecutionPlan for MysqlScanExec {
    fn name(&self) -> &str {
        "MysqlScanExec"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        vec![]
    }

    fn with_new_children(
        self: Arc<Self>,
        _children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        // Leaf plan — no children to swap.
        Ok(self)
    }

    fn execute(
        &self,
        _partition: usize,
        _context: Arc<TaskContext>,
    ) -> DfResult<SendableRecordBatchStream> {
        let pool = self.pool.clone();
        let sql = self.sql.clone();
        let schema = self.schema.clone();
        let columns = self.columns.clone();

        let fut = async move {
            let rows = sqlx::query(&sql)
                .fetch_all(&pool)
                .await
                .map_err(|e| DataFusionError::External(Box::new(e)))?;
            rows_to_record_batch(&rows, &columns, &schema)
        };

        let stream = futures::stream::once(fut);
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            self.schema.clone(),
            stream,
        )))
    }
}

fn rows_to_record_batch(
    rows: &[sqlx::mysql::MySqlRow],
    columns: &[crate::arrow_schema::MysqlColumn],
    schema: &Arc<ArrowSchema>,
) -> DfResult<RecordBatch> {
    let mut builders: Vec<ArrayBuilderHandle> = Vec::with_capacity(columns.len());
    for f in schema.fields() {
        builders.push(
            ArrayBuilderHandle::for_data_type(f.data_type(), rows.len())
                .map_err(|e| DataFusionError::External(Box::new(e)))?,
        );
    }

    for row in rows {
        for (col, builder) in columns.iter().zip(builders.iter_mut()) {
            append_value(row, col, builder).map_err(|e| DataFusionError::External(Box::new(e)))?;
        }
    }

    let arrays: Vec<datafusion::arrow::array::ArrayRef> =
        builders.into_iter().map(|b| b.finish()).collect();
    RecordBatch::try_new(schema.clone(), arrays).map_err(|e| DataFusionError::External(Box::new(e)))
}
