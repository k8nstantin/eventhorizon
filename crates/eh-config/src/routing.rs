//! Routing rules — the declarative `(intent shape → source)` map.
//!
//! Phase 1 supports the simplest possible rule shape: match on (entity,
//! action, optional mode) → name of the target source. Phase 5/6+ add
//! Cedar conditions and the predicate AST.

use eh_core::{Action, Mode};
use serde::{Deserialize, Serialize};

/// What to match in the intent.
///
/// `entity` is required; `action` defaults to "any" (None) when absent;
/// `mode` likewise. The match is most-specific-wins by virtue of rule
/// order: the first rule whose predicate covers the intent wins. Operators
/// should order rules from most to least specific in the YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingMatch {
    /// Logical entity name to match.
    pub entity: String,
    /// Action to match. `None` = any action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// Mode to match. `None` = any mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
}

impl RoutingMatch {
    /// True if this match's `entity` / `action` / `mode` predicate covers
    /// the given (entity, action, mode) tuple.
    #[must_use]
    pub fn covers(&self, entity: &str, action: Action, mode: Option<Mode>) -> bool {
        if self.entity != entity {
            return false;
        }
        if let Some(want) = self.action {
            if want != action {
                return false;
            }
        }
        if let Some(want) = self.mode {
            if Some(want) != mode {
                return false;
            }
        }
        true
    }
}

/// One routing rule: `when` predicate + `target` source name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingRule {
    /// Match predicate.
    pub when: RoutingMatch,
    /// Name of the target source (must exist in `sources:`).
    pub target: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_with_only_entity_covers_any_action_and_mode() {
        let m = RoutingMatch {
            entity: "Customer".to_string(),
            action: None,
            mode: None,
        };
        assert!(m.covers("Customer", Action::Read, None));
        assert!(m.covers("Customer", Action::Read, Some(Mode::Point)));
        assert!(m.covers("Customer", Action::Append, None));
        assert!(!m.covers("Order", Action::Read, None));
    }

    #[test]
    fn match_with_action_filters() {
        let m = RoutingMatch {
            entity: "Customer".to_string(),
            action: Some(Action::Read),
            mode: None,
        };
        assert!(m.covers("Customer", Action::Read, None));
        assert!(!m.covers("Customer", Action::Append, None));
    }

    #[test]
    fn match_with_mode_filters_only_when_mode_supplied() {
        let m = RoutingMatch {
            entity: "Customer".to_string(),
            action: Some(Action::Read),
            mode: Some(Mode::Point),
        };
        assert!(m.covers("Customer", Action::Read, Some(Mode::Point)));
        assert!(!m.covers("Customer", Action::Read, Some(Mode::Trend)));
        assert!(!m.covers("Customer", Action::Read, None));
    }

    #[test]
    fn rule_yaml_round_trip() {
        let yaml = r#"
when:
  entity: Customer
  action: read
target: fvp_mysql
"#;
        let r: RoutingRule = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(r.target, "fvp_mysql");
        assert_eq!(r.when.entity, "Customer");
        assert_eq!(r.when.action, Some(Action::Read));
        assert!(r.when.mode.is_none());
    }
}
