//! `Intent` — what an agent asks for. The smallest atomic unit of work.
//!
//! An intent names an entity (logical, agent-facing concept) and an action
//! (`read` or `append` for the FVP). Modifiers like `mode`, `fields`,
//! `filter`, and `payload` further shape the request. The router maps the
//! `(entity, action, mode)` tuple to a `(source, binding)` per the
//! declarative routing rules; the compiler then produces a backend-specific
//! plan from the bound entity.

use serde::{Deserialize, Serialize};

/// What the agent wants done with an entity.
///
/// For the Phase 1 FVP, only `Read` is exercised by the MySQL connector.
/// `Append` lands when the operator extends the eh_service grant to permit
/// INSERT on the relevant binding. `Update` and `Delete` are NOT actions
/// EventHorizon application code can authorize — they are intentionally
/// absent from the engine grant set (zero-trust §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Read rows from the entity. Maps to `SELECT` in SQL backends.
    Read,
    /// Append a new row (or new state version under SCD2). Maps to
    /// `INSERT` only — never `UPDATE` or `UPSERT`.
    Append,
}

/// Shape of the read. The router uses this to pick the right binding when
/// an entity is bound to multiple sources (e.g., OLTP for point reads,
/// lakehouse for analytical trend queries).
///
/// Phase 1 only exercises `Point`. The other variants exist in the type
/// surface so adding them in later phases does not require a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Single-row lookup (e.g., by primary key or unique field).
    Point,
    /// Time-series aggregation over a window. Used in Phase 7+ for
    /// trend/dashboard queries over Iceberg.
    Trend,
    /// Group-by aggregation.
    Aggregate,
    /// Windowed aggregation (sliding / tumbling windows).
    Window,
}

/// What an agent sends through the gateway.
///
/// Filter and payload are intentionally typed as `serde_json::Value` so the
/// surface stays connector-agnostic. The compiler validates and translates
/// them per the bound entity's field types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    /// What to do.
    pub action: Action,
    /// Which entity to act on. Must match the `name` of an `Entity` declared
    /// in the loaded configuration; the router refuses unknown entities.
    pub entity: String,
    /// Shape modifier for `Read` intents. `None` defaults to `Point`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Projection: which logical entity fields to include in the artifact.
    /// `None` means "all declared fields".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// Equality / range filter expressed as a JSON object keyed by entity
    /// field name. The compiler converts this to a parameterized backend
    /// query — never string-concatenated SQL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
    /// For `Append` intents: the row to insert. Keys are entity field names.
    /// Ignored for `Read` intents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl Intent {
    /// Returns the effective `Mode` for this intent. `Read` intents without
    /// an explicit mode default to `Point`; non-`Read` intents return `None`.
    #[must_use]
    pub fn effective_mode(&self) -> Option<Mode> {
        match self.action {
            Action::Read => Some(self.mode.unwrap_or(Mode::Point)),
            Action::Append => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn intent_read_point_round_trip() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".to_string(),
            mode: Some(Mode::Point),
            fields: Some(vec!["id".to_string(), "email".to_string()]),
            filter: Some(json!({ "id": "cust_1" })),
            payload: None,
        };
        let json = serde_json::to_value(&intent).unwrap();
        let back: Intent = serde_json::from_value(json).unwrap();
        assert_eq!(intent, back);
    }

    #[test]
    fn intent_append_round_trip() {
        let intent = Intent {
            action: Action::Append,
            entity: "ServerEvent".to_string(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!({ "kind": "boot", "host": "edge-3" })),
        };
        let json = serde_json::to_value(&intent).unwrap();
        let back: Intent = serde_json::from_value(json).unwrap();
        assert_eq!(intent, back);
    }

    #[test]
    fn action_serde_snake_case() {
        assert_eq!(serde_json::to_string(&Action::Read).unwrap(), "\"read\"");
        assert_eq!(
            serde_json::to_string(&Action::Append).unwrap(),
            "\"append\""
        );
    }

    #[test]
    fn mode_serde_snake_case() {
        assert_eq!(serde_json::to_string(&Mode::Point).unwrap(), "\"point\"");
        assert_eq!(serde_json::to_string(&Mode::Trend).unwrap(), "\"trend\"");
        assert_eq!(
            serde_json::to_string(&Mode::Aggregate).unwrap(),
            "\"aggregate\""
        );
        assert_eq!(serde_json::to_string(&Mode::Window).unwrap(), "\"window\"");
    }

    #[test]
    fn read_intent_without_mode_defaults_to_point() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".to_string(),
            mode: None,
            fields: None,
            filter: None,
            payload: None,
        };
        assert_eq!(intent.effective_mode(), Some(Mode::Point));
    }

    #[test]
    fn append_intent_has_no_effective_mode() {
        let intent = Intent {
            action: Action::Append,
            entity: "Customer".to_string(),
            mode: None,
            fields: None,
            filter: None,
            payload: Some(json!({})),
        };
        assert_eq!(intent.effective_mode(), None);
    }

    #[test]
    fn deserialise_minimal_intent_from_wire_json() {
        let s = r#"{"action":"read","entity":"Customer"}"#;
        let intent: Intent = serde_json::from_str(s).unwrap();
        assert_eq!(intent.action, Action::Read);
        assert_eq!(intent.entity, "Customer");
        assert_eq!(intent.mode, None);
        assert_eq!(intent.fields, None);
        assert_eq!(intent.filter, None);
        assert_eq!(intent.payload, None);
        assert_eq!(intent.effective_mode(), Some(Mode::Point));
    }

    #[test]
    fn serialise_intent_omits_none_fields() {
        let intent = Intent {
            action: Action::Read,
            entity: "Customer".to_string(),
            mode: None,
            fields: None,
            filter: None,
            payload: None,
        };
        let s = serde_json::to_string(&intent).unwrap();
        assert!(!s.contains("mode"));
        assert!(!s.contains("fields"));
        assert!(!s.contains("filter"));
        assert!(!s.contains("payload"));
    }
}
