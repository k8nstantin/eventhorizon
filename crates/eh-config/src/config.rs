//! The top-level YAML config and the `Entity` / `EntityBinding` shapes the
//! file uses (which match the `eh-core` types via `serde` interop).

use std::collections::BTreeMap;

use eh_core::{Entity, EntityBinding};
use serde::{Deserialize, Serialize};

use crate::routing::RoutingRule;
use crate::source::SourceConfig;

/// Currently-supported config version. Bumped when the YAML shape changes
/// in a way that requires a migration.
pub const SUPPORTED_VERSION: u32 = 1;

/// Top-level YAML configuration.
///
/// Use `loader::load_from_path` to read from disk; this struct itself is
/// just the on-the-wire shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Schema version of this config file. Must equal `SUPPORTED_VERSION`.
    pub version: u32,

    /// Named sources keyed by operator-chosen name.
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,

    /// Named entities (logical, agent-facing concepts) keyed by entity name.
    #[serde(default)]
    pub entities: BTreeMap<String, EntityFromYaml>,

    /// Entity → source bindings (ordered as declared).
    #[serde(default)]
    pub bindings: Vec<EntityBinding>,

    /// Routing rules (ordered as declared; first-match wins).
    #[serde(default)]
    pub routing: Vec<RoutingRule>,
}

/// YAML shape for an entity. The entity name lives outside the struct (it's
/// the map key in `entities:`); only the field list rides inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityFromYaml {
    /// Field declarations.
    #[serde(default)]
    pub fields: Vec<eh_core::EntityField>,
}

impl Config {
    /// Build `eh_core::Entity`s from the YAML shape, attaching each entity's
    /// map-key as its `name` field.
    pub(crate) fn entities_resolved(&self) -> BTreeMap<String, Entity> {
        self.entities
            .iter()
            .map(|(name, body)| {
                (
                    name.clone(),
                    Entity {
                        name: name.clone(),
                        fields: body.fields.clone(),
                    },
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh_core::{Action, EntityField, FieldMap, FieldType, Profile};
    use serde_yaml::Value;

    fn sample_yaml() -> &'static str {
        r#"
version: 1
sources:
  fvp_mysql:
    kind: mysql
    host: mysql
    port: 3306
    database: eh_demo
    username: eh_service
    password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
    ssl_mode: preferred
    max_pool_size: 8
entities:
  Customer:
    fields:
      - name: id
        data_type: uuid
        nullable: false
        pii: false
      - name: email
        data_type: string
        nullable: false
        pii: true
bindings:
  - entity: Customer
    source: fvp_mysql
    physical_table: eh_demo.customers
    profile: oltp
    supported_actions: [read]
    field_map:
      id: id
      email: email
routing:
  - when:
      entity: Customer
      action: read
    target: fvp_mysql
"#
    }

    #[test]
    fn parse_full_fvp_config() {
        let cfg: Config = serde_yaml::from_str(sample_yaml()).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.entities.len(), 1);
        assert_eq!(cfg.bindings.len(), 1);
        assert_eq!(cfg.routing.len(), 1);

        let src = cfg.sources.get("fvp_mysql").unwrap();
        assert_eq!(src.kind, "mysql");
        // The connector-specific keys live in the opaque mapping — `eh-config`
        // does not interpret them; the connector author parses them via
        // `SourceConfig::parse_into::<TheirConfigStruct>()`.
        assert_eq!(
            src.raw
                .get(Value::String("host".into()))
                .and_then(|v| v.as_str()),
            Some("mysql")
        );
        assert_eq!(
            src.raw
                .get(Value::String("port".into()))
                .and_then(|v| v.as_u64()),
            Some(3306)
        );
        assert_eq!(
            src.raw
                .get(Value::String("username".into()))
                .and_then(|v| v.as_str()),
            Some("eh_service")
        );
        // Password is stored as the raw `${ENV:...}` string in the opaque
        // mapping; the connector's typed config struct can hold a SecretRef
        // and parse it via Deserialize.
        assert_eq!(
            src.raw
                .get(Value::String("password".into()))
                .and_then(|v| v.as_str()),
            Some("${ENV:FVP_MYSQL_SERVICE_PASSWORD}")
        );

        let entities = cfg.entities_resolved();
        let customer = entities.get("Customer").unwrap();
        assert_eq!(customer.name, "Customer");
        assert_eq!(customer.fields.len(), 2);
        assert_eq!(customer.fields[0].name, "id");
        assert_eq!(customer.fields[0].data_type, FieldType::Uuid);

        let binding = &cfg.bindings[0];
        assert_eq!(binding.entity, "Customer");
        assert_eq!(binding.source, "fvp_mysql");
        assert_eq!(binding.physical_table, "eh_demo.customers");
        assert_eq!(binding.profile, Profile::Oltp);
        assert_eq!(binding.supported_actions, vec![Action::Read]);
        assert_eq!(binding.field_map.physical_for("email"), "email");

        let route = &cfg.routing[0];
        assert_eq!(route.target, "fvp_mysql");
        assert_eq!(route.when.entity, "Customer");
        assert_eq!(route.when.action, Some(Action::Read));
    }

    #[test]
    fn parse_minimal_config_with_just_version() {
        let yaml = "version: 1";
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.version, 1);
        assert!(cfg.sources.is_empty());
        assert!(cfg.entities.is_empty());
        assert!(cfg.bindings.is_empty());
        assert!(cfg.routing.is_empty());
    }

    #[test]
    fn entity_field_uses_data_type_key_not_type() {
        // Sanity: eh_core::EntityField calls the field `data_type`, NOT
        // `type` (which is a Rust keyword anyway). YAML must use `data_type`.
        let yaml = r#"
name: id
data_type: uuid
nullable: false
pii: false
"#;
        let f: EntityField = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(f.name, "id");
        assert_eq!(f.data_type, FieldType::Uuid);
    }

    #[test]
    fn entity_binding_round_trip_via_config() {
        let binding = EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Read],
            field_map: FieldMap::from_pairs([("id", "customer_id")]),
        };
        let yaml = serde_yaml::to_string(&binding).unwrap();
        let back: EntityBinding = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(binding, back);
    }
}
