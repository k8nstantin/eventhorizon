//! Parameterised INSERT builder.
//!
//! INSERT-only. The SCD2 triad (`valid_from`, `valid_to`, `is_current`) is
//! left to the database DEFAULTs declared in the locked schema — the
//! builder never references those columns explicitly. State changes are
//! pure INSERTs (zero-trust §10).

use eh_connector_api::ConnectorError;
use eh_core::{Entity, EntityBinding, Intent};
use serde_json::Value;
use uuid::Uuid;

use crate::ident::SafeIdent;
use crate::types::BindValue;

/// Output of the INSERT builder.
#[derive(Debug)]
pub(crate) struct BuiltInsert {
    /// Parameterised SQL text with `?` placeholders.
    pub sql: String,
    /// Bind values in placeholder order.
    pub binds: Vec<BindValue>,
}

/// Compile an `Append` intent into a parameterised INSERT.
///
/// Behaviour rules:
/// - Payload MUST be a JSON object keyed by **logical** entity field names.
/// - For each declared entity field, the connector looks for a value in
///   the payload. Missing values fail with `InvalidIntent` UNLESS:
///     * the field is the `id` and its type is `Uuid`, in which case a
///       UUIDv7 is generated server-side; OR
///     * the field is `nullable` per the entity declaration, in which case
///       NULL is bound.
/// - SCD2 columns (`valid_from`, `valid_to`, `is_current`) are NOT touched
///   — DB defaults handle them.
/// - Logical → physical name mapping comes from `binding.field_map`.
pub(crate) fn build_insert(
    binding: &EntityBinding,
    entity: &Entity,
    intent: &Intent,
) -> Result<BuiltInsert, ConnectorError> {
    let table = SafeIdent::table(&binding.physical_table)?;

    let payload = match intent.payload.as_ref() {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(ConnectorError::InvalidIntent(
                "append payload must be a JSON object keyed by logical field name".to_string(),
            ));
        }
        None => {
            return Err(ConnectorError::InvalidIntent(
                "append intent has no payload".to_string(),
            ));
        }
    };

    // For each declared entity field, determine the value to bind.
    let mut col_idents: Vec<SafeIdent> = Vec::with_capacity(entity.fields.len());
    let mut binds: Vec<BindValue> = Vec::with_capacity(entity.fields.len());

    for field in &entity.fields {
        let logical = field.name.as_str();
        let physical = binding.field_map.physical_for(logical);

        let value = match payload.get(logical) {
            Some(v) => BindValue::from_json(logical, field.data_type, v)?,
            None => {
                // Server-side generation of UUIDv7 ids when the payload
                // omits them — the only auto-fill exception. All other
                // missing-and-required fields are rejected.
                if logical == "id" && matches!(field.data_type, eh_core::FieldType::Uuid) {
                    BindValue::Uuid(Uuid::now_v7())
                } else if field.nullable {
                    BindValue::Null
                } else {
                    return Err(ConnectorError::InvalidIntent(format!(
                        "append payload missing required field {logical:?}"
                    )));
                }
            }
        };

        col_idents.push(SafeIdent::single(physical)?);
        binds.push(value);
    }

    // Reject unknown payload keys (defence in depth — keeps the wire shape
    // disciplined and surfaces typos early).
    for key in payload.keys() {
        if entity.field(key).is_none() {
            return Err(ConnectorError::InvalidIntent(format!(
                "append payload includes unknown field {key:?} for entity {}",
                entity.name
            )));
        }
    }

    // Sanity: at least one column to insert.
    if col_idents.is_empty() {
        return Err(ConnectorError::InvalidIntent(
            "entity has no fields; cannot build INSERT".to_string(),
        ));
    }

    let mut sql = String::with_capacity(64 + col_idents.len() * 16);
    sql.push_str("INSERT INTO ");
    sql.push_str(table.as_str());
    sql.push_str(" (");
    for (i, col) in col_idents.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push('`');
        sql.push_str(col.as_str());
        sql.push('`');
    }
    sql.push_str(") VALUES (");
    for i in 0..col_idents.len() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
    }
    sql.push(')');

    Ok(BuiltInsert { sql, binds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh_core::{Action, EntityField, FieldMap, FieldType, Profile};
    use serde_json::json;

    fn customer_entity() -> Entity {
        Entity {
            name: "Customer".to_string(),
            fields: vec![
                EntityField {
                    name: "id".into(),
                    data_type: FieldType::Uuid,
                    nullable: false,
                    pii: false,
                },
                EntityField {
                    name: "name".into(),
                    data_type: FieldType::String,
                    nullable: false,
                    pii: true,
                },
                EntityField {
                    name: "email".into(),
                    data_type: FieldType::String,
                    nullable: false,
                    pii: true,
                },
                EntityField {
                    name: "signup_at".into(),
                    data_type: FieldType::Timestamp,
                    nullable: false,
                    pii: false,
                },
                EntityField {
                    name: "ltv_usd".into(),
                    data_type: FieldType::Decimal,
                    nullable: false,
                    pii: false,
                },
            ],
        }
    }

    fn customer_binding() -> EntityBinding {
        EntityBinding {
            entity: "Customer".to_string(),
            source: "fvp_mysql".to_string(),
            physical_table: "eh_demo.customers".to_string(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Read, Action::Append],
            field_map: FieldMap::default(),
        }
    }

    fn intent_with_payload(payload: serde_json::Value) -> Intent {
        Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(payload),
        }
    }

    #[test]
    fn build_insert_emits_expected_sql_and_binds() {
        let intent = intent_with_payload(json!({
            "id": "01914a01-7001-7001-8001-000000000001",
            "name": "Alice",
            "email": "alice@example.com",
            "signup_at": "2024-09-01T09:00:00",
            "ltv_usd": "1250.00"
        }));
        let built = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap();
        assert_eq!(
            built.sql,
            "INSERT INTO eh_demo.customers (`id`, `name`, `email`, `signup_at`, `ltv_usd`) VALUES (?, ?, ?, ?, ?)"
        );
        assert_eq!(built.binds.len(), 5);
        assert!(matches!(built.binds[0], BindValue::Uuid(_)));
        assert!(matches!(built.binds[1], BindValue::String(ref s) if s == "Alice"));
        assert!(matches!(built.binds[3], BindValue::Timestamp(_)));
        assert!(matches!(built.binds[4], BindValue::Decimal(_)));
    }

    #[test]
    fn build_insert_generates_uuidv7_when_id_missing() {
        let intent = intent_with_payload(json!({
            "name": "Alice",
            "email": "alice@example.com",
            "signup_at": "2024-09-01T09:00:00",
            "ltv_usd": "1250.00"
        }));
        let built = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap();
        let id = match &built.binds[0] {
            BindValue::Uuid(u) => *u,
            _ => panic!("expected Uuid"),
        };
        // UUIDv7 has version 7 in the version nibble.
        assert_eq!(id.get_version_num(), 7);
    }

    #[test]
    fn build_insert_rejects_missing_required_non_id_field() {
        let intent = intent_with_payload(json!({
            "id": "01914a01-7001-7001-8001-000000000001",
            "name": "Alice",
            "email": "alice@example.com"
            // signup_at and ltv_usd missing — neither is nullable.
        }));
        let err = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(
            matches!(err, ConnectorError::InvalidIntent(ref s) if s.contains("signup_at") || s.contains("ltv_usd"))
        );
    }

    #[test]
    fn build_insert_rejects_unknown_payload_key() {
        let intent = intent_with_payload(json!({
            "id": "01914a01-7001-7001-8001-000000000001",
            "name": "Alice",
            "email": "alice@example.com",
            "signup_at": "2024-09-01T09:00:00",
            "ltv_usd": "1250.00",
            "frobnicator": "spy"
        }));
        let err = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(ref s) if s.contains("frobnicator")));
    }

    #[test]
    fn build_insert_rejects_missing_payload() {
        let intent = Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: None,
        };
        let err = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(_)));
    }

    #[test]
    fn build_insert_rejects_non_object_payload() {
        let intent = intent_with_payload(json!("not-an-object"));
        let err = build_insert(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(_)));
    }

    #[test]
    fn build_insert_uses_field_map_for_physical_column_names() {
        let mut binding = customer_binding();
        binding.field_map = FieldMap::from_pairs([
            ("id", "customer_id"),
            ("name", "full_name"),
            ("email", "contact_email"),
            ("signup_at", "created_at"),
            ("ltv_usd", "lifetime_value_usd"),
        ]);
        let intent = intent_with_payload(json!({
            "id": "01914a01-7001-7001-8001-000000000001",
            "name": "Alice",
            "email": "alice@example.com",
            "signup_at": "2024-09-01T09:00:00",
            "ltv_usd": "1250.00"
        }));
        let built = build_insert(&binding, &customer_entity(), &intent).unwrap();
        assert!(built.sql.contains("`customer_id`"));
        assert!(built.sql.contains("`full_name`"));
        assert!(built.sql.contains("`contact_email`"));
        assert!(built.sql.contains("`created_at`"));
        assert!(built.sql.contains("`lifetime_value_usd`"));
    }

    #[test]
    fn build_insert_nullable_field_omitted_binds_null() {
        let entity = Entity {
            name: "X".into(),
            fields: vec![
                EntityField {
                    name: "id".into(),
                    data_type: FieldType::Uuid,
                    nullable: false,
                    pii: false,
                },
                EntityField {
                    name: "optional".into(),
                    data_type: FieldType::String,
                    nullable: true,
                    pii: false,
                },
            ],
        };
        let binding = EntityBinding {
            entity: "X".into(),
            source: "s".into(),
            physical_table: "xs".into(),
            profile: Profile::Oltp,
            supported_actions: vec![Action::Append],
            field_map: FieldMap::default(),
        };
        let intent = Intent {
            action: Action::Append,
            entity: "X".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!({})),
        };
        let built = build_insert(&binding, &entity, &intent).unwrap();
        assert_eq!(built.binds.len(), 2);
        assert!(matches!(built.binds[0], BindValue::Uuid(_))); // server-generated id
        assert!(matches!(built.binds[1], BindValue::Null));
    }
}
