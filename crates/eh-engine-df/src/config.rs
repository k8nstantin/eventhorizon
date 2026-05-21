//! Operator-tunable policy for the DataFusion engine.
//!
//! Parsed from the YAML `datafusion:` section. Every knob has a sensible
//! default; an operator can omit the entire block and still get a safe
//! posture (deny DDL/DML, modest limits, projection + predicate pushdown
//! enabled, PII hidden from `/v1/schema`).
//!
//! Hot-reloadable via the same `ConfigCache` pattern `eh-config` uses;
//! see PR 1.8b for the swap-on-reload plumbing.

use serde::{Deserialize, Serialize};

/// Top-level `datafusion:` config block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EngineConfig {
    /// What statements / patterns to refuse at parse time.
    #[serde(default)]
    pub policy: PolicyConfig,
    /// Per-query resource caps.
    #[serde(default)]
    pub limits: LimitsConfig,
    /// What kinds of pushdown the planner is allowed to request from
    /// connectors. Connectors' own `ConnectorCaps` still cap the actual
    /// pushdown — this section gates the upper bound.
    #[serde(default)]
    pub pushdown: PushdownConfig,
    /// What `GET /v1/schema` exposes to the agent / copilot.
    #[serde(default)]
    pub schema_exposure: SchemaExposureConfig,
    /// DataFusion's optimizer knobs.
    #[serde(default)]
    pub optimizer: OptimizerConfig,
}

/// Statement-class denylist + raw-SQL regex denylist (belt-and-braces).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Statement classes refused at AST parse time. The default denies
    /// everything that mutates schema or rows out-of-band. The agent's
    /// only mutating path is `Action::Append` through a binding that
    /// declares it; raw SQL `INSERT` is allowed only via
    /// `access_mode: sql_passthrough` (PR 1.9).
    #[serde(default = "default_deny_statements")]
    pub deny_statements: Vec<StatementClass>,
    /// Raw-SQL regexes that fail BEFORE the parser sees the statement.
    /// Belt-and-braces against parser bugs; not the primary gate.
    #[serde(default)]
    pub deny_regex: Vec<String>,
    /// If true, refuse `SELECT *` or any `SELECT` without an explicit
    /// `FROM` clause referencing a known table. Default false.
    #[serde(default)]
    pub require_explicit_table_refs: bool,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            deny_statements: default_deny_statements(),
            deny_regex: Vec::new(),
            require_explicit_table_refs: false,
        }
    }
}

fn default_deny_statements() -> Vec<StatementClass> {
    vec![
        StatementClass::Ddl,
        StatementClass::Delete,
        StatementClass::Update,
        StatementClass::Truncate,
        StatementClass::Merge,
        StatementClass::Replace,
        StatementClass::Grant,
        StatementClass::Revoke,
        StatementClass::Set,
    ]
}

/// Statement classes the AST walker classifies an incoming SQL statement
/// into. The denylist is matched against these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatementClass {
    /// `CREATE`, `ALTER`, `DROP` of any object.
    Ddl,
    /// `DELETE ...`
    Delete,
    /// `UPDATE ...`
    Update,
    /// `TRUNCATE ...`
    Truncate,
    /// `MERGE ...` / `UPSERT ...` — never allowed (zero-trust §10).
    Merge,
    /// `REPLACE INTO ...` — MySQL-flavoured upsert.
    Replace,
    /// `GRANT ...`
    Grant,
    /// `REVOKE ...`
    Revoke,
    /// `SET ...` — session vars, role changes, sql_mode tweaks. Default-deny.
    Set,
}

/// Per-query resource caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Max wall-clock seconds before the query is cancelled.
    #[serde(default = "default_max_query_seconds")]
    pub max_query_seconds: u32,
    /// Hard cap on rows returned to the agent. The execution plan is
    /// wrapped in a LIMIT node if the agent did not specify one smaller.
    #[serde(default = "default_max_rows_returned")]
    pub max_rows_returned: u64,
    /// Memory budget for the in-process query execution.
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_query_seconds: default_max_query_seconds(),
            max_rows_returned: default_max_rows_returned(),
            max_memory_mb: default_max_memory_mb(),
        }
    }
}

fn default_max_query_seconds() -> u32 {
    30
}
fn default_max_rows_returned() -> u64 {
    10_000
}
fn default_max_memory_mb() -> u32 {
    512
}

/// Pushdown caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushdownConfig {
    /// Allow the planner to push projections (column lists) into the
    /// connector. Always safe; default `true`.
    #[serde(default = "default_true")]
    pub projection: bool,
    /// Maximum predicate pushdown level the planner may request.
    #[serde(default)]
    pub predicate: PushdownPredicate,
}

impl Default for PushdownConfig {
    fn default() -> Self {
        Self {
            projection: true,
            predicate: PushdownPredicate::default(),
        }
    }
}

/// Predicate-pushdown ceiling. The connector's own `ConnectorCaps` may
/// declare less; the planner picks the lower bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PushdownPredicate {
    /// Never push predicates; in-process filter only.
    None,
    /// Allow inexact pushdown (post-filter required).
    Inexact,
    /// Allow exact pushdown when the connector advertises it.
    #[default]
    Exact,
}

/// What `GET /v1/schema` includes. Tightens or relaxes what the agent
/// and the planned copilot (Gemma, Phase 9) can see about the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaExposureConfig {
    /// Include fields flagged `pii: true` in `/v1/schema` responses.
    /// Default `false` — operators opt in.
    #[serde(default)]
    pub include_pii_fields: bool,
    /// Include internal / control-plane tables. Default `false`.
    #[serde(default)]
    pub include_internal_tables: bool,
    /// Include lineage hints (which entity is backed by which Iceberg
    /// table, etc.). Default `true`.
    #[serde(default = "default_true")]
    pub include_lineage_hints: bool,
}

impl Default for SchemaExposureConfig {
    fn default() -> Self {
        Self {
            include_pii_fields: false,
            include_internal_tables: false,
            include_lineage_hints: true,
        }
    }
}

/// DataFusion optimizer toggles. Mostly safe defaults; exposed so the
/// operator can disable a misbehaving pass without re-deploying the
/// gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Allow partition / file pruning passes.
    #[serde(default = "default_true")]
    pub enable_pruning: bool,
    /// Allow join-order rewrites.
    #[serde(default = "default_true")]
    pub enable_join_reordering: bool,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            enable_pruning: true,
            enable_join_reordering: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_default_config_round_trips_yaml() {
        let cfg = EngineConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let back: EngineConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn empty_yaml_loads_safe_defaults() {
        let cfg: EngineConfig = serde_yaml::from_str("{}").unwrap();
        // Default policy refuses DDL/DML/MERGE/etc.
        assert!(cfg.policy.deny_statements.contains(&StatementClass::Ddl));
        assert!(cfg.policy.deny_statements.contains(&StatementClass::Delete));
        assert!(cfg.policy.deny_statements.contains(&StatementClass::Update));
        assert!(cfg.policy.deny_statements.contains(&StatementClass::Merge));
        assert_eq!(cfg.policy.deny_regex.len(), 0);
        assert!(!cfg.policy.require_explicit_table_refs);

        // Modest limits.
        assert_eq!(cfg.limits.max_query_seconds, 30);
        assert_eq!(cfg.limits.max_rows_returned, 10_000);
        assert_eq!(cfg.limits.max_memory_mb, 512);

        // Pushdown on by default.
        assert!(cfg.pushdown.projection);
        assert!(matches!(cfg.pushdown.predicate, PushdownPredicate::Exact));

        // PII hidden by default; lineage shown.
        assert!(!cfg.schema_exposure.include_pii_fields);
        assert!(!cfg.schema_exposure.include_internal_tables);
        assert!(cfg.schema_exposure.include_lineage_hints);

        // Optimizer on by default.
        assert!(cfg.optimizer.enable_pruning);
        assert!(cfg.optimizer.enable_join_reordering);
    }

    #[test]
    fn deny_statements_can_be_overridden_to_relax() {
        // Operator deploying a read-only-but-introspect-friendly gateway
        // could choose to allow SET for session vars (e.g., timezone).
        let yaml = r#"
policy:
  deny_statements: [ddl, delete, update, truncate, merge, replace]
"#;
        let cfg: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.policy.deny_statements.contains(&StatementClass::Set));
        assert!(cfg.policy.deny_statements.contains(&StatementClass::Ddl));
    }

    #[test]
    fn deny_regex_round_trips() {
        let yaml = r#"
policy:
  deny_regex:
    - "(?i)\\bdrop\\s+table\\b"
    - "(?i)\\btruncate\\b"
"#;
        let cfg: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.policy.deny_regex.len(), 2);
        assert!(cfg.policy.deny_regex[0].contains("drop"));
    }

    #[test]
    fn limits_partial_override() {
        let yaml = "limits:\n  max_query_seconds: 5\n";
        let cfg: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.limits.max_query_seconds, 5);
        // Unmentioned fields take the default.
        assert_eq!(cfg.limits.max_rows_returned, 10_000);
        assert_eq!(cfg.limits.max_memory_mb, 512);
    }

    #[test]
    fn statement_class_serialises_lowercase() {
        assert_eq!(
            serde_yaml::to_string(&StatementClass::Ddl).unwrap().trim(),
            "ddl"
        );
        assert_eq!(
            serde_yaml::to_string(&StatementClass::Truncate)
                .unwrap()
                .trim(),
            "truncate"
        );
    }

    #[test]
    fn pushdown_predicate_serde() {
        assert_eq!(
            serde_yaml::to_string(&PushdownPredicate::None)
                .unwrap()
                .trim(),
            "none"
        );
        let p: PushdownPredicate = serde_yaml::from_str("inexact").unwrap();
        assert!(matches!(p, PushdownPredicate::Inexact));
    }

    #[test]
    fn schema_exposure_operator_opts_into_pii() {
        let yaml = "schema_exposure:\n  include_pii_fields: true\n";
        let cfg: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.schema_exposure.include_pii_fields);
        // Other fields take default.
        assert!(!cfg.schema_exposure.include_internal_tables);
        assert!(cfg.schema_exposure.include_lineage_hints);
    }
}
