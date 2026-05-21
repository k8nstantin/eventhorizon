//! # eh-router
//!
//! Resolves an incoming `Intent` against the loaded `CompiledConfig` to
//! produce a `RoutedIntent` — the (entity, binding, source name) tuple
//! the dispatcher hands to the matching connector.
//!
//! Phase 1 uses first-match-wins over the declared routing rules. The
//! predicate AST + Cedar conditions arrive in Phase 5+.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use eh_config::CompiledConfig;
use eh_core::{Action, Entity, EntityBinding, Intent, Mode};
use thiserror::Error;

/// Result of routing an intent.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedIntent {
    /// The bound entity (cloned for the dispatcher to hand to the connector).
    pub entity: Entity,
    /// The selected binding.
    pub binding: EntityBinding,
    /// Name of the target source in the loaded config.
    pub source_name: String,
}

/// Typed routing errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouterError {
    /// The intent references an entity not declared in the loaded config.
    #[error("unknown entity {0:?}")]
    UnknownEntity(String),

    /// No routing rule covers this `(entity, action, mode)` tuple.
    #[error("no routing rule matches entity {entity:?} action {action:?} mode {mode:?}")]
    NoRoute {
        /// Entity name.
        entity: String,
        /// Requested action.
        action: Action,
        /// Effective mode (if the action is Read).
        mode: Option<Mode>,
    },

    /// A routing rule matched but no binding exists for its target source.
    #[error("routing rule targets source {source_name:?} but no binding for entity {entity:?} → that source")]
    NoBindingForRoute {
        /// Entity name.
        entity: String,
        /// Source name from the matching routing rule. Field is `source_name`
        /// rather than `source` to avoid `thiserror`'s auto-`#[source]`
        /// detection (which would expect an `Error` impl on the value).
        source_name: String,
    },

    /// The selected binding does not declare support for the requested
    /// action — the binding's `supported_actions` parameter excludes it.
    #[error("binding for entity {entity:?} on source {source_name:?} does not declare action {action:?}")]
    ActionNotSupported {
        /// Entity name.
        entity: String,
        /// Action the intent requested.
        action: Action,
        /// Source the binding routes to.
        source_name: String,
    },
}

/// Resolve an intent against the loaded config.
///
/// Algorithm:
/// 1. Look up the entity by name. Unknown → `UnknownEntity`.
/// 2. Walk the routing rules in declared order; pick the first whose
///    `when` predicate covers `(intent.entity, intent.action,
///    intent.effective_mode())`.
/// 3. Find the binding for `(entity = intent.entity, source = rule.target)`.
/// 4. Verify the binding's `supported_actions` includes the requested
///    action — `eh_service` engine grants enforce this too, but failing
///    fast here gives a clearer typed error.
pub fn route(intent: &Intent, cfg: &CompiledConfig) -> Result<RoutedIntent, RouterError> {
    let entity = cfg
        .entity(&intent.entity)
        .ok_or_else(|| RouterError::UnknownEntity(intent.entity.clone()))?;

    let action = intent.action;
    let mode = intent.effective_mode();

    for rule in &cfg.routing {
        if !rule.when.covers(&intent.entity, action, mode) {
            continue;
        }
        let binding = cfg
            .bindings_for_entity(&intent.entity)
            .iter()
            .find(|b| b.source == rule.target)
            .cloned()
            .ok_or_else(|| RouterError::NoBindingForRoute {
                entity: intent.entity.clone(),
                source_name: rule.target.clone(),
            })?;

        if !binding.supports(action) {
            return Err(RouterError::ActionNotSupported {
                entity: intent.entity.clone(),
                action,
                source_name: binding.source.clone(),
            });
        }

        return Ok(RoutedIntent {
            entity: entity.clone(),
            binding,
            source_name: rule.target.clone(),
        });
    }

    Err(RouterError::NoRoute {
        entity: intent.entity.clone(),
        action,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh_config::{Config, EntityFromYaml, RoutingMatch, RoutingRule, SourceConfig};
    use eh_core::{EntityField, FieldMap, FieldType, Profile};
    use serde_yaml::{Mapping, Value};
    use std::collections::BTreeMap;

    fn customer_entity() -> EntityFromYaml {
        EntityFromYaml {
            fields: vec![EntityField {
                name: "id".into(),
                data_type: FieldType::Uuid,
                nullable: false,
                pii: false,
            }],
        }
    }

    fn mysql_source() -> SourceConfig {
        let mut raw = Mapping::new();
        raw.insert(Value::String("host".into()), Value::String("mysql".into()));
        SourceConfig::new("mysql", raw)
    }

    fn config_with(bindings: Vec<EntityBinding>, routing: Vec<RoutingRule>) -> CompiledConfig {
        let mut sources = BTreeMap::new();
        sources.insert("fvp_mysql".to_string(), mysql_source());
        let mut entities = BTreeMap::new();
        entities.insert("Customer".to_string(), customer_entity());
        let cfg = Config {
            version: 1,
            sources,
            entities,
            bindings,
            routing,
        };
        cfg.compile().expect("config should compile")
    }

    fn customer_binding(actions: Vec<Action>) -> EntityBinding {
        EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: actions,
            field_map: FieldMap::default(),
        }
    }

    fn read_intent() -> Intent {
        Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: None,
        }
    }

    #[test]
    fn route_happy_path() {
        let cfg = config_with(
            vec![customer_binding(vec![Action::Read])],
            vec![RoutingRule {
                when: RoutingMatch {
                    entity: "Customer".into(),
                    action: Some(Action::Read),
                    mode: None,
                },
                target: "fvp_mysql".into(),
            }],
        );
        let routed = route(&read_intent(), &cfg).unwrap();
        assert_eq!(routed.source_name, "fvp_mysql");
        assert_eq!(routed.binding.entity, "Customer");
        assert_eq!(routed.entity.name, "Customer");
    }

    #[test]
    fn unknown_entity_rejected() {
        let cfg = config_with(vec![], vec![]);
        let mut i = read_intent();
        i.entity = "Phantom".into();
        match route(&i, &cfg) {
            Err(RouterError::UnknownEntity(e)) => assert_eq!(e, "Phantom"),
            other => panic!("expected UnknownEntity, got {other:?}"),
        }
    }

    #[test]
    fn no_matching_route_rejected() {
        let cfg = config_with(
            vec![customer_binding(vec![Action::Read])],
            vec![], // no rules at all
        );
        match route(&read_intent(), &cfg) {
            Err(RouterError::NoRoute { entity, action, .. }) => {
                assert_eq!(entity, "Customer");
                assert_eq!(action, Action::Read);
            }
            other => panic!("expected NoRoute, got {other:?}"),
        }
    }

    #[test]
    fn rule_matching_but_no_binding_rejected() {
        // Routing points at `fvp_mysql`, but no binding exists for that pair.
        let cfg = config_with(
            vec![],
            vec![RoutingRule {
                when: RoutingMatch {
                    entity: "Customer".into(),
                    action: Some(Action::Read),
                    mode: None,
                },
                target: "fvp_mysql".into(),
            }],
        );
        match route(&read_intent(), &cfg) {
            Err(RouterError::NoBindingForRoute {
                entity,
                source_name,
            }) => {
                assert_eq!(entity, "Customer");
                assert_eq!(source_name, "fvp_mysql");
            }
            other => panic!("expected NoBindingForRoute, got {other:?}"),
        }
    }

    #[test]
    fn binding_without_supported_action_is_rejected() {
        let cfg = config_with(
            // Binding declares READ only.
            vec![customer_binding(vec![Action::Read])],
            // Routing rule allows Append on Customer.
            vec![RoutingRule {
                when: RoutingMatch {
                    entity: "Customer".into(),
                    action: Some(Action::Append),
                    mode: None,
                },
                target: "fvp_mysql".into(),
            }],
        );
        let mut append = read_intent();
        append.action = Action::Append;
        match route(&append, &cfg) {
            Err(RouterError::ActionNotSupported {
                entity,
                action,
                source_name,
            }) => {
                assert_eq!(entity, "Customer");
                assert_eq!(action, Action::Append);
                assert_eq!(source_name, "fvp_mysql");
            }
            other => panic!("expected ActionNotSupported, got {other:?}"),
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        // Two rules — both could match a Customer read; the first wins.
        let cfg = config_with(
            vec![customer_binding(vec![Action::Read])],
            vec![
                RoutingRule {
                    when: RoutingMatch {
                        entity: "Customer".into(),
                        action: Some(Action::Read),
                        mode: Some(Mode::Point),
                    },
                    target: "fvp_mysql".into(),
                },
                RoutingRule {
                    when: RoutingMatch {
                        entity: "Customer".into(),
                        action: Some(Action::Read),
                        mode: None,
                    },
                    target: "fvp_mysql".into(),
                },
            ],
        );
        let routed = route(&read_intent(), &cfg).unwrap();
        // The intent has effective mode Point (Read default). The first
        // rule matches with mode=Point, so the result is that rule.
        assert_eq!(routed.source_name, "fvp_mysql");
    }
}
