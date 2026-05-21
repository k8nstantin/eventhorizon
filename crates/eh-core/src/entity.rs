//! Semantic entities and their physical bindings.
//!
//! An `Entity` is what an agent sees — `Customer`, `Order`, etc. Every entity
//! has a list of typed `EntityField`s. An `EntityBinding` maps the entity to
//! a physical table in a specific source, with a per-field column-name
//! mapping.
//!
//! Entities and bindings mirror the `eh_control.entities` /
//! `eh_control.entity_fields` / `eh_control.entity_bindings` /
//! `eh_control.entity_field_bindings` tables (SCHEMA.md §3.8 / §3.9 / §3.10).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The data type of an entity field, mirroring `eh_control.entity_fields.data_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    /// Bounded text (e.g. VARCHAR / TEXT length-bound).
    String,
    /// Unbounded text payload (e.g. TEXT).
    Text,
    /// 32-bit signed integer.
    Int,
    /// 64-bit signed integer.
    BigInt,
    /// Arbitrary-precision decimal number.
    Decimal,
    /// 64-bit floating point.
    Float,
    /// Boolean.
    Bool,
    /// UUIDv7 — typed end-to-end per §14.
    Uuid,
    /// Timezone-aware timestamp.
    Timestamp,
    /// Opaque JSON payload (rare; used for archived intents, telemetry tails).
    Json,
    /// Opaque binary payload.
    Binary,
}

/// A single declared field on an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityField {
    /// Logical field name as agents see it (e.g. `email`).
    pub name: String,
    /// Type the gateway enforces end-to-end.
    pub data_type: FieldType,
    /// Whether the field accepts NULL.
    #[serde(default)]
    pub nullable: bool,
    /// Whether the field carries PII (informational tag for compliance).
    #[serde(default)]
    pub pii: bool,
}

/// A logical, agent-facing entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Entity name (e.g. `Customer`). Unique per tenant.
    pub name: String,
    /// Ordered field declarations.
    pub fields: Vec<EntityField>,
}

impl Entity {
    /// Look up a field by name.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&EntityField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Returns true if the entity declares every name in `names`.
    #[must_use]
    pub fn has_all_fields<S: AsRef<str>>(&self, names: &[S]) -> bool {
        names.iter().all(|n| self.field(n.as_ref()).is_some())
    }
}

/// Workload profile for a binding. The router uses this when an entity is
/// bound to multiple sources to pick the right one per intent mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Low-latency point reads + writes against a transactional store.
    Oltp,
    /// High-throughput analytical queries against a lakehouse or warehouse.
    Analytical,
    /// Long-tail historical data, infrequently queried.
    Archival,
    /// Vector-similarity / nearest-neighbor lookups (RAG / vector stores).
    Similarity,
}

/// Per-field name mapping inside a binding. Logical field name → physical
/// column name on the bound table. Stored sorted by logical name for stable
/// serialization.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldMap {
    /// Logical → physical name map.
    pub mapping: BTreeMap<String, String>,
}

impl FieldMap {
    /// Construct a `FieldMap` from a sequence of (logical, physical) pairs.
    #[must_use]
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut mapping = BTreeMap::new();
        for (k, v) in pairs {
            mapping.insert(k.into(), v.into());
        }
        Self { mapping }
    }

    /// Look up the physical column for a logical field. If absent, returns
    /// the logical name unchanged (so 1:1 mappings need no explicit entry).
    #[must_use]
    pub fn physical_for<'a>(&'a self, logical: &'a str) -> &'a str {
        self.mapping
            .get(logical)
            .map_or(logical, std::string::String::as_str)
    }
}

/// Maps an entity (and its fields) to a physical table in a specific source.
///
/// Mirrors `eh_control.entity_bindings` + `eh_control.entity_field_bindings`
/// + `eh_control.entity_binding_actions` (SCHEMA.md §3.10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityBinding {
    /// Entity name this binding is for.
    pub entity: String,
    /// Source name this binding routes to.
    pub source: String,
    /// Backing physical table (schema-qualified, e.g. `public.customers` or
    /// `warehouse.customers_history`).
    pub physical_table: String,
    /// Workload profile.
    pub profile: Profile,
    /// Per-binding allowed actions. Mirrors `eh_control.entity_binding_actions`.
    pub supported_actions: Vec<super::Action>,
    /// Logical → physical field-name map.
    #[serde(default)]
    pub field_map: FieldMap,
}

impl EntityBinding {
    /// Returns true if the binding declares support for the given action.
    #[must_use]
    pub fn supports(&self, action: super::Action) -> bool {
        self.supported_actions.contains(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;

    fn customer() -> Entity {
        Entity {
            name: "Customer".to_string(),
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
                EntityField {
                    name: "signup_at".to_string(),
                    data_type: FieldType::Timestamp,
                    nullable: false,
                    pii: false,
                },
            ],
        }
    }

    #[test]
    fn entity_field_lookup() {
        let e = customer();
        assert_eq!(e.field("email").unwrap().data_type, FieldType::String);
        assert!(e.field("nope").is_none());
        assert!(e.has_all_fields(&["id", "email"]));
        assert!(!e.has_all_fields(&["id", "nope"]));
    }

    #[test]
    fn entity_round_trip() {
        let e = customer();
        let json = serde_json::to_value(&e).unwrap();
        let back: Entity = serde_json::from_value(json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn field_type_serde_snake_case() {
        assert_eq!(serde_json::to_string(&FieldType::Uuid).unwrap(), "\"uuid\"");
        assert_eq!(
            serde_json::to_string(&FieldType::Timestamp).unwrap(),
            "\"timestamp\""
        );
        assert_eq!(
            serde_json::to_string(&FieldType::BigInt).unwrap(),
            "\"big_int\""
        );
    }

    #[test]
    fn field_map_falls_back_to_logical_name() {
        let m = FieldMap::from_pairs([("id", "customer_id"), ("email", "contact_email")]);
        assert_eq!(m.physical_for("id"), "customer_id");
        assert_eq!(m.physical_for("email"), "contact_email");
        // Unmapped logical name returns itself unchanged.
        assert_eq!(m.physical_for("signup_at"), "signup_at");
    }

    #[test]
    fn field_map_round_trip() {
        let m = FieldMap::from_pairs([("a", "alpha"), ("b", "beta")]);
        let s = serde_json::to_string(&m).unwrap();
        // `transparent` serialisation flattens to the inner BTreeMap.
        assert_eq!(s, r#"{"a":"alpha","b":"beta"}"#);
        let back: FieldMap = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn binding_round_trip() {
        let b = EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Read],
            field_map: FieldMap::from_pairs([("id", "customer_id")]),
        };
        let json = serde_json::to_value(&b).unwrap();
        let back: EntityBinding = serde_json::from_value(json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn binding_supports_action() {
        let b = EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Read],
            field_map: FieldMap::default(),
        };
        assert!(b.supports(Action::Read));
        assert!(!b.supports(Action::Append));
    }

    #[test]
    fn profile_serde_snake_case() {
        assert_eq!(serde_json::to_string(&Profile::Oltp).unwrap(), "\"oltp\"");
        assert_eq!(
            serde_json::to_string(&Profile::Analytical).unwrap(),
            "\"analytical\""
        );
        assert_eq!(
            serde_json::to_string(&Profile::Similarity).unwrap(),
            "\"similarity\""
        );
    }
}
