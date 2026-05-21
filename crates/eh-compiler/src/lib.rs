//! # eh-compiler
//!
//! **Phase 1 scope**: pre-execution intent validation. Verifies an intent's
//! projection, filter, and payload reference only declared entity fields
//! before dispatching to a connector. Returns a typed `CompileError` for
//! each kind of shape violation so the edge can surface a structured
//! response without hitting the backend.
//!
//! **Phase 4+ scope**: this crate gains the DataFusion `LogicalPlan`
//! compiler that the federation story is built on. The Phase 1 validator
//! lives in the `validate` function and remains the cheap pre-flight check.
//!
//! Why the SQL itself isn't built here in Phase 1: the FVP MySQL connector
//! (`eh-connector-mysql`) emits parameterised SELECT / INSERT directly
//! from the intent + binding, because there's exactly one source per
//! intent and DataFusion's plan/optimiser cost isn't justified yet. When
//! Phase 4 lands and we need cross-source plans, the connector falls back
//! to executing a DataFusion `LogicalPlan` built here. The `Connector`
//! trait stays the same — only the implementation strategy widens.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use eh_core::{Action, Entity, Intent};
use serde_json::Value;
use thiserror::Error;

/// Typed pre-flight validation errors. Each is mapped to a stable
/// `ErrorCode::InvalidIntent` at the edge.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompileError {
    /// The intent's `entity` does not match the entity passed for
    /// validation. Catches programming errors in the dispatch path.
    #[error("intent entity {intent_entity:?} does not match passed entity {entity_name:?}")]
    EntityMismatch {
        /// Entity name the intent claims.
        intent_entity: String,
        /// Entity name the validator was called with.
        entity_name: String,
    },

    /// The intent's projection references a field the entity does not
    /// declare.
    #[error("projection references unknown field {field:?} on entity {entity:?}")]
    UnknownProjectionField {
        /// Field name from the projection list.
        field: String,
        /// Entity name.
        entity: String,
    },

    /// The intent's filter references a field the entity does not declare.
    #[error("filter references unknown field {field:?} on entity {entity:?}")]
    UnknownFilterField {
        /// Field name from the filter.
        field: String,
        /// Entity name.
        entity: String,
    },

    /// The intent's filter is supplied but is not a JSON object.
    #[error("filter must be a JSON object keyed by field name")]
    FilterNotObject,

    /// An `Append` intent has no payload.
    #[error("append intent has no payload")]
    AppendMissingPayload,

    /// An `Append` intent's payload is supplied but is not a JSON object.
    #[error("append payload must be a JSON object keyed by field name")]
    PayloadNotObject,

    /// An `Append` intent's payload references a field the entity does
    /// not declare.
    #[error("payload references unknown field {field:?} on entity {entity:?}")]
    UnknownPayloadField {
        /// Field name from the payload.
        field: String,
        /// Entity name.
        entity: String,
    },

    /// A `Read` intent is forbidden from carrying a payload.
    #[error("read intent must not include a payload")]
    ReadHasPayload,
}

/// Pre-flight validation of an intent against the entity it routes to.
///
/// Catches structural violations before the connector compiles a SQL
/// statement. Returns `Ok(())` if the intent is shape-valid; the
/// connector then proceeds and produces parameterised SQL.
pub fn validate(intent: &Intent, entity: &Entity) -> Result<(), CompileError> {
    if intent.entity != entity.name {
        return Err(CompileError::EntityMismatch {
            intent_entity: intent.entity.clone(),
            entity_name: entity.name.clone(),
        });
    }

    if let Some(fields) = intent.fields.as_ref() {
        for f in fields {
            if entity.field(f).is_none() {
                return Err(CompileError::UnknownProjectionField {
                    field: f.clone(),
                    entity: entity.name.clone(),
                });
            }
        }
    }

    if let Some(filter) = intent.filter.as_ref() {
        match filter {
            Value::Object(map) => {
                for key in map.keys() {
                    if entity.field(key).is_none() {
                        return Err(CompileError::UnknownFilterField {
                            field: key.clone(),
                            entity: entity.name.clone(),
                        });
                    }
                }
            }
            _ => return Err(CompileError::FilterNotObject),
        }
    }

    match intent.action {
        Action::Read => {
            if intent.payload.is_some() {
                return Err(CompileError::ReadHasPayload);
            }
        }
        Action::Append => match intent.payload.as_ref() {
            None => return Err(CompileError::AppendMissingPayload),
            Some(Value::Object(map)) => {
                for key in map.keys() {
                    if entity.field(key).is_none() {
                        return Err(CompileError::UnknownPayloadField {
                            field: key.clone(),
                            entity: entity.name.clone(),
                        });
                    }
                }
            }
            Some(_) => return Err(CompileError::PayloadNotObject),
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh_core::{EntityField, FieldType, Mode};
    use serde_json::json;

    fn customer() -> Entity {
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
                    name: "email".into(),
                    data_type: FieldType::String,
                    nullable: false,
                    pii: true,
                },
            ],
        }
    }

    fn read_intent() -> Intent {
        Intent {
            action: Action::Read,
            entity: "Customer".into(),
            mode: Some(Mode::Point),
            fields: Some(vec!["id".into(), "email".into()]),
            filter: Some(json!({ "id": "x" })),
            payload: None,
        }
    }

    #[test]
    fn read_intent_valid() {
        validate(&read_intent(), &customer()).unwrap();
    }

    #[test]
    fn read_intent_with_payload_rejected() {
        let mut i = read_intent();
        i.payload = Some(json!({}));
        assert_eq!(
            validate(&i, &customer()).unwrap_err(),
            CompileError::ReadHasPayload
        );
    }

    #[test]
    fn entity_mismatch_rejected() {
        let mut i = read_intent();
        i.entity = "Phantom".into();
        match validate(&i, &customer()).unwrap_err() {
            CompileError::EntityMismatch {
                intent_entity,
                entity_name,
            } => {
                assert_eq!(intent_entity, "Phantom");
                assert_eq!(entity_name, "Customer");
            }
            other => panic!("expected EntityMismatch, got {other:?}"),
        }
    }

    #[test]
    fn unknown_projection_field_rejected() {
        let mut i = read_intent();
        i.fields = Some(vec!["id".into(), "ghost".into()]);
        match validate(&i, &customer()).unwrap_err() {
            CompileError::UnknownProjectionField { field, entity } => {
                assert_eq!(field, "ghost");
                assert_eq!(entity, "Customer");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn unknown_filter_field_rejected() {
        let mut i = read_intent();
        i.filter = Some(json!({ "ghost": "x" }));
        match validate(&i, &customer()).unwrap_err() {
            CompileError::UnknownFilterField { field, entity } => {
                assert_eq!(field, "ghost");
                assert_eq!(entity, "Customer");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn non_object_filter_rejected() {
        let mut i = read_intent();
        i.filter = Some(json!("not-an-object"));
        assert_eq!(
            validate(&i, &customer()).unwrap_err(),
            CompileError::FilterNotObject
        );
    }

    #[test]
    fn append_without_payload_rejected() {
        let i = Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: None,
        };
        assert_eq!(
            validate(&i, &customer()).unwrap_err(),
            CompileError::AppendMissingPayload
        );
    }

    #[test]
    fn append_with_non_object_payload_rejected() {
        let i = Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!(42)),
        };
        assert_eq!(
            validate(&i, &customer()).unwrap_err(),
            CompileError::PayloadNotObject
        );
    }

    #[test]
    fn append_with_unknown_payload_field_rejected() {
        let i = Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!({ "id": "x", "ghost": "y" })),
        };
        match validate(&i, &customer()).unwrap_err() {
            CompileError::UnknownPayloadField { field, entity } => {
                assert_eq!(field, "ghost");
                assert_eq!(entity, "Customer");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn append_with_valid_payload_accepted() {
        let i = Intent {
            action: Action::Append,
            entity: "Customer".into(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!({ "id": "x", "email": "y@z.com" })),
        };
        validate(&i, &customer()).unwrap();
    }
}
