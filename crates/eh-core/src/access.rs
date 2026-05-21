//! Access-scope types.
//!
//! Two distinct scopes (decided 2026-05-21):
//!
//! ## `SourceAccessScope` — set on each source in YAML
//!
//! Defines what tables / schemas / databases the CONNECTOR exposes to
//! DataFusion at all. This is the FIRST defence gate: out-of-scope
//! tables don't exist as far as the gateway is concerned. A connector
//! implementation MUST refuse to register anything outside its scope
//! with the `SessionContext`.
//!
//! ```yaml
//! sources:
//!   fvp_mysql:
//!     kind: mysql
//!     # ... connection config ...
//!     access:
//!       scope: tables
//!       schema: eh_demo
//!       tables: [customers, orders]
//! ```
//!
//! ## `BindingAccessMode` + `BindingAccessScope` — set on each binding
//!
//! Defines the per-entity access posture for the agent:
//! - `Intent` mode: agent submits JSON intents; `access` is implicit
//!   (only the binding's `physical_table` is reachable).
//! - `SqlPassthrough` mode: agent submits raw SQL; `access` allow-lists
//!   the tables that SQL may touch. MUST be a subset of the source's
//!   `SourceAccessScope`.
//!
//! ```yaml
//! bindings:
//!   - entity: AnalyticsWorkbench
//!     source: fvp_mysql
//!     access_mode: sql_passthrough
//!     access:
//!       tables: [customers, orders]
//! ```
//!
//! Together with engine grants, these form three independent defence
//! gates. See memory: eventhorizon-access-modes.

use serde::{Deserialize, Serialize};

/// What a CONNECTOR exposes to the always-on DataFusion engine. The
/// connector ONLY registers tables in scope; out-of-scope tables are
/// invisible to DF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum SourceAccessScope {
    /// Expose exactly one table.
    Table {
        /// Database / schema name on the backend.
        schema: String,
        /// Table name within that schema.
        table: String,
    },
    /// Expose an explicit allow-list of tables within one schema.
    Tables {
        /// Database / schema name on the backend.
        schema: String,
        /// Allow-listed table names within that schema.
        tables: Vec<String>,
    },
    /// Expose every table in one schema. The connector enumerates them
    /// via the backend's `information_schema` (or equivalent) at startup.
    WholeSchema {
        /// Database / schema name on the backend.
        schema: String,
    },
    /// Expose every schema + table the connector can see (subject to
    /// engine grants). Multiple databases can be listed when the backend
    /// supports cross-database access.
    WholeDatabase {
        /// One or more database / catalog names.
        databases: Vec<String>,
    },
}

impl SourceAccessScope {
    /// True if `(schema, table)` is in scope. The agent / engine path
    /// uses this to refuse anything outside, BEFORE issuing a query.
    #[must_use]
    pub fn contains(&self, schema: &str, table: &str) -> bool {
        match self {
            Self::Table {
                schema: s,
                table: t,
            } => s == schema && t == table,
            Self::Tables {
                schema: s,
                tables: ts,
            } => s == schema && ts.iter().any(|t| t == table),
            Self::WholeSchema { schema: s } => s == schema,
            Self::WholeDatabase { databases } => databases.iter().any(|d| d == schema),
        }
    }

    /// All `(schema, table)` pairs the connector should enumerate from
    /// the backend to populate its catalog. For the unbounded variants
    /// (`WholeSchema`, `WholeDatabase`) the connector queries
    /// `information_schema` at startup and registers what it finds.
    /// Returns `None` to signal "ask the backend; everything in this
    /// schema/database is in scope."
    #[must_use]
    pub fn explicit_tables(&self) -> Option<Vec<(String, String)>> {
        match self {
            Self::Table { schema, table } => Some(vec![(schema.clone(), table.clone())]),
            Self::Tables { schema, tables } => {
                Some(tables.iter().map(|t| (schema.clone(), t.clone())).collect())
            }
            Self::WholeSchema { .. } | Self::WholeDatabase { .. } => None,
        }
    }
}

/// How the agent expresses requests against this binding. See memory
/// entry `eventhorizon-access-modes` for the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BindingAccessMode {
    /// Agent submits a JSON `Intent`. Gateway compiles it to a
    /// parameterised SQL plan internally. Lowest risk; default.
    #[default]
    Intent,
    /// Agent submits raw SQL via `Action::ExecuteSql`. Subject to the
    /// AST-level firewall + the binding's `BindingAccessScope`.
    SqlPassthrough,
}

/// Per-binding access-scope. ONLY meaningful for `BindingAccessMode::SqlPassthrough`;
/// `Intent` mode is implicitly scoped to `EntityBinding::physical_table`.
///
/// MUST be a subset of the parent source's `SourceAccessScope` — the
/// config validator refuses a wider binding scope at load time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum BindingAccessScope {
    /// Single-table scope.
    Table {
        /// Schema on the backend.
        schema: String,
        /// Table name.
        table: String,
    },
    /// Explicit allow-list within one schema.
    Tables {
        /// Schema on the backend.
        schema: String,
        /// Allow-listed table names.
        tables: Vec<String>,
    },
    /// Whole schema. Only allowed when the source also exposes the whole
    /// schema (or wider).
    WholeSchema {
        /// Schema on the backend.
        schema: String,
    },
}

impl BindingAccessScope {
    /// True if `(schema, table)` is reachable from this binding.
    #[must_use]
    pub fn contains(&self, schema: &str, table: &str) -> bool {
        match self {
            Self::Table {
                schema: s,
                table: t,
            } => s == schema && t == table,
            Self::Tables {
                schema: s,
                tables: ts,
            } => s == schema && ts.iter().any(|t| t == table),
            Self::WholeSchema { schema: s } => s == schema,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_scope_table_round_trip() {
        let s = SourceAccessScope::Table {
            schema: "eh_demo".into(),
            table: "customers".into(),
        };
        let yaml = serde_yaml::to_string(&s).unwrap();
        assert!(yaml.contains("scope: table"));
        let back: SourceAccessScope = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn source_scope_tables_contains() {
        let s = SourceAccessScope::Tables {
            schema: "eh_demo".into(),
            tables: vec!["customers".into(), "orders".into()],
        };
        assert!(s.contains("eh_demo", "customers"));
        assert!(s.contains("eh_demo", "orders"));
        assert!(!s.contains("eh_demo", "secrets"));
        assert!(!s.contains("other_schema", "customers"));
    }

    #[test]
    fn source_scope_whole_schema_contains_any_table_in_schema() {
        let s = SourceAccessScope::WholeSchema {
            schema: "eh_demo".into(),
        };
        assert!(s.contains("eh_demo", "anything"));
        assert!(!s.contains("eh_admin", "anything"));
    }

    #[test]
    fn source_scope_whole_database_contains_listed_schemas() {
        let s = SourceAccessScope::WholeDatabase {
            databases: vec!["eh_demo".into(), "eh_analytics".into()],
        };
        assert!(s.contains("eh_demo", "x"));
        assert!(s.contains("eh_analytics", "y"));
        assert!(!s.contains("eh_admin", "x"));
    }

    #[test]
    fn explicit_tables_returns_some_for_bounded_scopes() {
        let s = SourceAccessScope::Tables {
            schema: "eh_demo".into(),
            tables: vec!["customers".into(), "orders".into()],
        };
        let got = s.explicit_tables().unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.contains(&("eh_demo".into(), "customers".into())));

        let s2 = SourceAccessScope::WholeSchema {
            schema: "eh_demo".into(),
        };
        assert!(s2.explicit_tables().is_none());
    }

    #[test]
    fn binding_access_mode_yaml_lowercase() {
        let m: BindingAccessMode = serde_yaml::from_str("intent").unwrap();
        assert_eq!(m, BindingAccessMode::Intent);
        let m: BindingAccessMode = serde_yaml::from_str("sql_passthrough").unwrap();
        assert_eq!(m, BindingAccessMode::SqlPassthrough);
    }

    #[test]
    fn binding_scope_subset_of_source_scope_check() {
        // Logical subset check the loader will perform at compile time.
        let source = SourceAccessScope::Tables {
            schema: "eh_demo".into(),
            tables: vec!["customers".into(), "orders".into()],
        };
        let binding = BindingAccessScope::Table {
            schema: "eh_demo".into(),
            table: "customers".into(),
        };
        // Every table the binding allows must be in the source's scope.
        assert!(source.contains("eh_demo", "customers"));
        assert!(binding.contains("eh_demo", "customers"));
        assert!(!binding.contains("eh_demo", "orders")); // narrower than source
    }
}
