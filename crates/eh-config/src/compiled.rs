//! `CompiledConfig` — the validated, indexed in-memory form the router and
//! compiler operate on. Produced by `Config::compile()`.

use std::collections::{BTreeMap, HashMap};

use eh_core::{Entity, EntityBinding};

use crate::config::{Config, SUPPORTED_VERSION};
use crate::errors::{ConfigError, ConfigResult};
use crate::routing::RoutingRule;
use crate::source::SourceConfig;

/// A validated, indexed view of the config ready for hot-path use.
///
/// Holds:
/// - all sources keyed by name
/// - all entities keyed by name
/// - bindings indexed by entity name (one entity may have multiple bindings
///   — Phase 1 has just one per entity; Phase 4+ uses multiple for OLTP +
///   analytical profiles)
/// - routing rules in declared order (first-match wins)
///
/// `CompiledConfig` is the value the router and compiler read on every
/// intent. In production each pod holds `Arc<ArcSwap<CompiledConfig>>` for
/// lock-free hot-path access and NOTIFY-driven hot reload (Phase 6).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledConfig {
    /// All sources, keyed by operator-chosen name.
    pub sources: BTreeMap<String, SourceConfig>,
    /// All entities, keyed by entity name.
    pub entities: BTreeMap<String, Entity>,
    /// Bindings indexed by entity name. Each `Vec<EntityBinding>` keeps the
    /// declaration order from YAML so router matches are stable.
    pub bindings_by_entity: HashMap<String, Vec<EntityBinding>>,
    /// Routing rules in declared order.
    pub routing: Vec<RoutingRule>,
}

impl CompiledConfig {
    /// Look up the named source.
    #[must_use]
    pub fn source(&self, name: &str) -> Option<&SourceConfig> {
        self.sources.get(name)
    }

    /// Look up the named entity.
    #[must_use]
    pub fn entity(&self, name: &str) -> Option<&Entity> {
        self.entities.get(name)
    }

    /// Get all bindings for an entity in declared order.
    #[must_use]
    pub fn bindings_for_entity(&self, name: &str) -> &[EntityBinding] {
        self.bindings_by_entity
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

impl Config {
    /// Validate the parsed config and produce a `CompiledConfig` for runtime use.
    ///
    /// Validation rules (each violation is its own typed `ConfigError`):
    /// - `version` must equal `SUPPORTED_VERSION`.
    /// - Every binding's `entity` must exist in `entities:`.
    /// - Every binding's `source` must exist in `sources:`.
    /// - Every key in a binding's `field_map` must be a declared field of
    ///   the binding's entity.
    /// - Every routing rule's `target` must be a known source name.
    /// - Every routing rule's `when.entity` must be a known entity name.
    pub fn compile(self) -> ConfigResult<CompiledConfig> {
        if self.version != SUPPORTED_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: self.version,
                supported: SUPPORTED_VERSION,
            });
        }

        let entities = self.entities_resolved();
        let sources = self.sources;

        // Validate bindings.
        let mut bindings_by_entity: HashMap<String, Vec<EntityBinding>> = HashMap::new();
        for binding in &self.bindings {
            let entity = entities
                .get(&binding.entity)
                .ok_or_else(|| ConfigError::UnknownEntityInBinding(binding.entity.clone()))?;
            if !sources.contains_key(&binding.source) {
                return Err(ConfigError::UnknownSourceInBinding(binding.source.clone()));
            }
            for logical in binding.field_map.mapping.keys() {
                if entity.field(logical).is_none() {
                    return Err(ConfigError::UnknownFieldInBinding {
                        entity: binding.entity.clone(),
                        field: logical.clone(),
                    });
                }
            }
            bindings_by_entity
                .entry(binding.entity.clone())
                .or_default()
                .push(binding.clone());
        }

        // Validate routing.
        for rule in &self.routing {
            if !entities.contains_key(&rule.when.entity) {
                return Err(ConfigError::UnknownEntityInRoute(rule.when.entity.clone()));
            }
            if !sources.contains_key(&rule.target) {
                return Err(ConfigError::UnknownTargetInRoute(rule.target.clone()));
            }
        }

        Ok(CompiledConfig {
            sources,
            entities,
            bindings_by_entity,
            routing: self.routing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EntityFromYaml;
    use crate::secret::SecretRef;
    use crate::source::{MysqlSourceConfig, MysqlSslMode};
    use eh_core::{Action, EntityField, FieldMap, FieldType, Profile};

    fn mk_source() -> (String, SourceConfig) {
        (
            "fvp_mysql".to_string(),
            SourceConfig::Mysql(MysqlSourceConfig {
                host: "mysql".into(),
                port: 3306,
                database: "eh_demo".into(),
                username: "eh_service".into(),
                password: SecretRef::Env("FVP_MYSQL_SERVICE_PASSWORD".into()),
                ssl_mode: MysqlSslMode::Preferred,
                max_pool_size: 8,
            }),
        )
    }

    fn mk_entity() -> (String, EntityFromYaml) {
        (
            "Customer".to_string(),
            EntityFromYaml {
                fields: vec![
                    EntityField {
                        name: "id".to_string(),
                        data_type: FieldType::Uuid,
                        nullable: false,
                        pii: false,
                    },
                    EntityField {
                        name: "email".to_string(),
                        data_type: FieldType::String,
                        nullable: false,
                        pii: true,
                    },
                ],
            },
        )
    }

    fn mk_binding() -> EntityBinding {
        EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Read],
            field_map: FieldMap::from_pairs([("id", "id"), ("email", "email")]),
        }
    }

    fn mk_config() -> Config {
        let (sn, sc) = mk_source();
        let (en, ev) = mk_entity();
        let mut sources = BTreeMap::new();
        sources.insert(sn, sc);
        let mut entities = BTreeMap::new();
        entities.insert(en, ev);
        Config {
            version: 1,
            sources,
            entities,
            bindings: vec![mk_binding()],
            routing: vec![RoutingRule {
                when: crate::routing::RoutingMatch {
                    entity: "Customer".to_string(),
                    action: Some(Action::Read),
                    mode: None,
                },
                target: "fvp_mysql".to_string(),
            }],
        }
    }

    #[test]
    fn compile_happy_path() {
        let compiled = mk_config().compile().unwrap();
        assert_eq!(compiled.entities.len(), 1);
        assert_eq!(compiled.sources.len(), 1);
        assert_eq!(compiled.bindings_for_entity("Customer").len(), 1);
        assert_eq!(compiled.routing.len(), 1);
        assert!(compiled.source("fvp_mysql").is_some());
        assert!(compiled.entity("Customer").is_some());
    }

    #[test]
    fn compile_rejects_unsupported_version() {
        let mut c = mk_config();
        c.version = 9;
        let err = c.compile().unwrap_err();
        assert!(matches!(err, ConfigError::UnsupportedVersion { .. }));
    }

    #[test]
    fn compile_rejects_binding_with_unknown_entity() {
        let mut c = mk_config();
        c.bindings[0].entity = "Frobnicator".to_string();
        let err = c.compile().unwrap_err();
        assert!(matches!(err, ConfigError::UnknownEntityInBinding(ref e) if e == "Frobnicator"));
    }

    #[test]
    fn compile_rejects_binding_with_unknown_source() {
        let mut c = mk_config();
        c.bindings[0].source = "ghost_source".to_string();
        let err = c.compile().unwrap_err();
        assert!(matches!(err, ConfigError::UnknownSourceInBinding(ref s) if s == "ghost_source"));
    }

    #[test]
    fn compile_rejects_binding_with_unknown_field_in_map() {
        let mut c = mk_config();
        c.bindings[0].field_map = FieldMap::from_pairs([("id", "id"), ("nope", "not_a_field")]);
        let err = c.compile().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::UnknownFieldInBinding { ref entity, ref field }
            if entity == "Customer" && field == "nope"
        ));
    }

    #[test]
    fn compile_rejects_routing_to_unknown_target() {
        let mut c = mk_config();
        c.routing[0].target = "ghost".to_string();
        let err = c.compile().unwrap_err();
        assert!(matches!(err, ConfigError::UnknownTargetInRoute(ref s) if s == "ghost"));
    }

    #[test]
    fn compile_rejects_routing_with_unknown_entity() {
        let mut c = mk_config();
        c.routing[0].when.entity = "Phantom".to_string();
        let err = c.compile().unwrap_err();
        assert!(matches!(err, ConfigError::UnknownEntityInRoute(ref s) if s == "Phantom"));
    }
}
