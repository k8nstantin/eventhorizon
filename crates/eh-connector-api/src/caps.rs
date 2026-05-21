//! Connector capability declaration.
//!
//! A connector's `capabilities()` MUST be honest. The conformance suite
//! (Phase 3) verifies declared capabilities match observed behaviour;
//! lying here breaks the router's correctness assumptions.

use serde::{Deserialize, Serialize};

/// How exactly a connector can satisfy `WHERE` clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushdownLevel {
    /// Connector cannot push predicates down — the gateway filters in-process.
    None,
    /// Connector can push some predicates but the result may include rows
    /// outside the predicate; the gateway must post-filter.
    Inexact,
    /// Connector pushes all supported predicates exactly; no post-filter
    /// needed for those predicate kinds.
    Exact,
}

/// What a connector declares it can do.
///
/// `supports_read` / `supports_append`: whether the connector implements
/// `execute_read` / `execute_append`. The binding's `supported_actions` in
/// the YAML config further narrows what reaches the connector — a connector
/// that supports both can be exposed for read-only via the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorCaps {
    /// Connector implements `execute_read`.
    pub supports_read: bool,
    /// Connector implements `execute_append` (INSERT only — never UPDATE).
    pub supports_append: bool,
    /// Pushdown level for WHERE clauses.
    pub predicate_pushdown: PushdownLevel,
    /// Whether the connector projects columns server-side. If false, the
    /// connector returns all columns and the gateway projects in-process.
    pub projection_pushdown: bool,
    /// Whether the connector streams row batches instead of materialising
    /// the full result set in memory. Phase 7 (Iceberg) is where streaming
    /// matters; Phase 1 connectors set this to false safely.
    pub streaming: bool,
}

impl ConnectorCaps {
    /// Conservative defaults — connector declares no capability. Connectors
    /// override this in `capabilities()` to declare what they actually
    /// support.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            supports_read: false,
            supports_append: false,
            predicate_pushdown: PushdownLevel::None,
            projection_pushdown: false,
            streaming: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_declares_nothing() {
        let c = ConnectorCaps::none();
        assert!(!c.supports_read);
        assert!(!c.supports_append);
        assert_eq!(c.predicate_pushdown, PushdownLevel::None);
        assert!(!c.projection_pushdown);
        assert!(!c.streaming);
    }

    #[test]
    fn pushdown_level_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&PushdownLevel::None).unwrap(),
            "\"none\""
        );
        assert_eq!(
            serde_json::to_string(&PushdownLevel::Inexact).unwrap(),
            "\"inexact\""
        );
        assert_eq!(
            serde_json::to_string(&PushdownLevel::Exact).unwrap(),
            "\"exact\""
        );
    }
}
