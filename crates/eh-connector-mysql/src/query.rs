//! Parameterised SELECT builder.
//!
//! Takes an `Intent` + an `EntityBinding` + the bound `Entity` and produces
//! the SQL text + the typed `BindValue`s to feed to sqlx. Identifiers
//! (table, columns) come from the validated config; values are parameter
//! placeholders, never interpolated.

use eh_connector_api::ConnectorError;
use eh_core::{Entity, EntityBinding, FieldType, Intent};
use serde_json::Value;

use crate::ident::SafeIdent;
use crate::types::BindValue;

/// Output of the SELECT builder.
#[derive(Debug)]
pub(crate) struct BuiltQuery {
    /// Parameterised SQL text with `?` placeholders.
    pub sql: String,
    /// Logical → physical mapping for the selected columns, in the order
    /// they appear in `sql`. Used to decode rows back into logical field
    /// names for the artifact.
    pub projection: Vec<ProjectedColumn>,
    /// Bind values in the order their placeholders appear in `sql`.
    pub binds: Vec<BindValue>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectedColumn {
    /// Logical entity field name.
    pub logical: String,
    /// Physical column name on the bound table.
    pub physical: String,
    /// Declared type of the logical field.
    pub field_type: FieldType,
}

/// Compile a `Read` intent into a parameterised SELECT.
pub(crate) fn build_select(
    binding: &EntityBinding,
    entity: &Entity,
    intent: &Intent,
) -> Result<BuiltQuery, ConnectorError> {
    let table = SafeIdent::table(&binding.physical_table)?;

    let projected = resolve_projection(binding, entity, intent.fields.as_deref())?;
    if projected.is_empty() {
        return Err(ConnectorError::InvalidIntent(
            "projection resolves to no columns".to_string(),
        ));
    }

    let mut sql = String::with_capacity(64 + projected.len() * 16);
    sql.push_str("SELECT ");
    for (i, col) in projected.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        let safe = SafeIdent::single(&col.physical)?;
        sql.push('`');
        sql.push_str(safe.as_str());
        sql.push('`');
    }
    sql.push_str(" FROM ");
    sql.push_str(table.as_str());

    let mut binds: Vec<BindValue> = Vec::new();
    let mut where_clauses: Vec<String> = Vec::new();
    if let Some(Value::Object(filter)) = intent.filter.as_ref() {
        // Sorted for deterministic SQL ordering — easier to read in logs
        // and easier to assert in tests.
        let mut keys: Vec<&String> = filter.keys().collect();
        keys.sort();
        for key in keys {
            let value = &filter[key];
            let field = entity.field(key).ok_or_else(|| {
                ConnectorError::InvalidIntent(format!(
                    "filter references unknown field {key:?} on entity {}",
                    entity.name
                ))
            })?;
            let physical = binding.field_map.physical_for(key);
            let safe_col = SafeIdent::single(physical)?;
            where_clauses.push(format!("`{}` = ?", safe_col.as_str()));
            binds.push(BindValue::from_json(&field.name, field.data_type, value)?);
        }
    } else if intent.filter.is_some() {
        return Err(ConnectorError::InvalidIntent(
            "filter must be a JSON object keyed by entity field name".to_string(),
        ));
    }

    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }

    // RLS / SCD2 filtering: only currently-active rows. The customers
    // table has `is_current` per the locked FVP schema; the entity carries
    // the rest of the fields. Phase 1 limits all SELECTs to current rows.
    let is_current_phys = binding.field_map.physical_for("is_current");
    // Only add the `is_current = 1` filter if the bound table actually has
    // such a column. We detect this conservatively: only add the clause if
    // the binding does not map `is_current` away and the entity does NOT
    // declare it (i.e., it's a back-end-only column).
    if entity.field("is_current").is_none() {
        let safe_is_current = SafeIdent::single(is_current_phys)?;
        if where_clauses.is_empty() {
            sql.push_str(" WHERE ");
        } else {
            sql.push_str(" AND ");
        }
        sql.push('`');
        sql.push_str(safe_is_current.as_str());
        sql.push_str("` = 1");
    }

    // Phase 1 limit. Operators with analytic intents fall through to
    // Iceberg in later phases. Until then we cap at 1000 to avoid an
    // accidental full-table scan.
    sql.push_str(" LIMIT 1000");

    Ok(BuiltQuery {
        sql,
        projection: projected,
        binds,
    })
}

fn resolve_projection(
    binding: &EntityBinding,
    entity: &Entity,
    requested: Option<&[String]>,
) -> Result<Vec<ProjectedColumn>, ConnectorError> {
    let logical_fields: Vec<&str> = if let Some(fields) = requested {
        // Validate each requested field exists on the entity.
        for f in fields {
            if entity.field(f).is_none() {
                return Err(ConnectorError::InvalidIntent(format!(
                    "projection references unknown field {f:?} on entity {}",
                    entity.name
                )));
            }
        }
        fields.iter().map(String::as_str).collect()
    } else {
        entity.fields.iter().map(|f| f.name.as_str()).collect()
    };

    let mut out = Vec::with_capacity(logical_fields.len());
    for logical in logical_fields {
        let field = entity.field(logical).expect("validated above");
        let physical = binding.field_map.physical_for(logical).to_string();
        out.push(ProjectedColumn {
            logical: logical.to_string(),
            physical,
            field_type: field.data_type,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh_core::{Action, EntityField, FieldMap, FieldType, Mode, Profile};
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
            supported_actions: vec![Action::Read],
            field_map: FieldMap::from_pairs([
                ("id", "id"),
                ("name", "name"),
                ("email", "email"),
                ("signup_at", "signup_at"),
                ("ltv_usd", "ltv_usd"),
            ]),
        }
    }

    #[test]
    fn select_all_fields_when_projection_omitted() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: None,
            filter: None,
            payload: None,
        };
        let built = build_select(&customer_binding(), &customer_entity(), &intent).unwrap();
        // Order should follow the entity's field declaration order.
        let cols: Vec<_> = built
            .projection
            .iter()
            .map(|c| c.logical.as_str())
            .collect();
        assert_eq!(cols, vec!["id", "name", "email", "signup_at", "ltv_usd"]);
        // No filter binds; is_current=1 added because the entity does not declare it.
        assert!(built.binds.is_empty());
        assert!(built.sql.starts_with(
            "SELECT `id`, `name`, `email`, `signup_at`, `ltv_usd` FROM eh_demo.customers"
        ));
        assert!(built.sql.contains("WHERE `is_current` = 1"));
        assert!(built.sql.ends_with("LIMIT 1000"));
    }

    #[test]
    fn select_projected_fields_in_requested_order() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: Some(vec!["email".into(), "id".into()]),
            filter: None,
            payload: None,
        };
        let built = build_select(&customer_binding(), &customer_entity(), &intent).unwrap();
        let cols: Vec<_> = built
            .projection
            .iter()
            .map(|c| c.logical.as_str())
            .collect();
        assert_eq!(cols, vec!["email", "id"]);
        assert!(built.sql.starts_with("SELECT `email`, `id` FROM"));
    }

    #[test]
    fn select_with_filter_binds_parameterised() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: Some(vec!["id".into(), "email".into()]),
            filter: Some(json!({
                "email": "alice@example.com",
                "id":    "01914a01-7001-7001-8001-000000000001"
            })),
            payload: None,
        };
        let built = build_select(&customer_binding(), &customer_entity(), &intent).unwrap();
        // Filter clauses are emitted in sorted key order: email, id.
        assert!(built
            .sql
            .contains("WHERE `email` = ? AND `id` = ? AND `is_current` = 1"));
        assert_eq!(built.binds.len(), 2);
        assert!(matches!(built.binds[0], BindValue::String(ref s) if s == "alice@example.com"));
        assert!(matches!(built.binds[1], BindValue::Uuid(_)));
    }

    #[test]
    fn select_rejects_filter_referencing_unknown_field() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: None,
            filter: Some(json!({ "frobnicator": "x" })),
            payload: None,
        };
        let err = build_select(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(_)));
    }

    #[test]
    fn select_rejects_projection_referencing_unknown_field() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: Some(vec!["mystery".into()]),
            filter: None,
            payload: None,
        };
        let err = build_select(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(_)));
    }

    #[test]
    fn select_rejects_filter_that_is_not_object() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: None,
            filter: Some(json!("not-an-object")),
            payload: None,
        };
        let err = build_select(&customer_binding(), &customer_entity(), &intent).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidIntent(_)));
    }

    #[test]
    fn select_uses_field_map_to_translate_logical_to_physical() {
        let mut binding = customer_binding();
        binding.field_map = FieldMap::from_pairs([
            ("id", "customer_id"),
            ("email", "contact_email"),
            ("name", "full_name"),
            ("signup_at", "created_at"),
            ("ltv_usd", "lifetime_value_usd"),
        ]);
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: Some(vec!["id".into(), "email".into()]),
            filter: Some(json!({ "id": "01914a01-7001-7001-8001-000000000001" })),
            payload: None,
        };
        let built = build_select(&binding, &customer_entity(), &intent).unwrap();
        assert!(built.sql.contains("`customer_id`"));
        assert!(built.sql.contains("`contact_email`"));
        assert!(built.sql.contains("WHERE `customer_id` = ?"));
    }
}
