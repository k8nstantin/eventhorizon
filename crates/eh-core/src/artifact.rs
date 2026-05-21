//! The shape of what the gateway returns to the caller.
//!
//! An `Artifact` is a homogeneous batch of rows produced by executing an
//! intent. Each `ArtifactRow` is a JSON object keyed by **logical entity
//! field name** (the agent's view) — never by physical column name (which
//! is the connector's view). The compiler is responsible for translating
//! between the two.
//!
//! Phase 1 ships `Vec<ArtifactRow>` as the wire shape. Phase 7 introduces
//! streaming via Arrow `RecordBatch` for large analytical scans; the
//! `Artifact` envelope stays the same.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row of an artifact, keyed by **logical entity field name**.
///
/// The map order is preserved by `serde_json::Map`, so projections stay in
/// the order the intent requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArtifactRow(pub serde_json::Map<String, serde_json::Value>);

impl ArtifactRow {
    /// Construct an empty row.
    #[must_use]
    pub fn new() -> Self {
        Self(serde_json::Map::new())
    }

    /// Insert a field value. Returns the previous value if the field was
    /// already set.
    pub fn insert<S: Into<String>>(
        &mut self,
        field: S,
        value: serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.0.insert(field.into(), value)
    }

    /// Number of fields in the row.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if the row has no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for ArtifactRow {
    fn default() -> Self {
        Self::new()
    }
}

/// The complete result of an intent execution.
///
/// Carries the rows plus minimal connector-provenance for observability
/// (so traces and logs can attribute a result back to a source without
/// the agent needing to know the source identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Rows in projection order.
    pub rows: Vec<ArtifactRow>,
    /// Kind of source that produced the rows (e.g. `"mysql"`, `"postgres"`,
    /// `"iceberg"`). Surfaced for telemetry / debug; not used for routing.
    pub source_kind: String,
    /// Stable identifier of the source row in `eh_control.sources`, if known.
    /// `None` in Phase 1 when the control plane has no rows yet (YAML-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
}

impl Artifact {
    /// Number of rows in the artifact.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// True if the artifact has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn artifact_row_insert_get_round_trip() {
        let mut row = ArtifactRow::new();
        assert!(row.is_empty());
        row.insert("id", json!("cust_1"));
        row.insert("email", json!("alice@example.com"));
        assert_eq!(row.len(), 2);
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(
            json,
            json!({ "id": "cust_1", "email": "alice@example.com" })
        );
        let back: ArtifactRow = serde_json::from_value(json).unwrap();
        assert_eq!(row, back);
    }

    #[test]
    fn artifact_with_one_row_round_trip() {
        let mut row = ArtifactRow::new();
        row.insert("id", json!("cust_1"));
        let artifact = Artifact {
            rows: vec![row],
            source_kind: "mysql".to_string(),
            source_id: None,
        };
        let json = serde_json::to_value(&artifact).unwrap();
        let back: Artifact = serde_json::from_value(json).unwrap();
        assert_eq!(artifact, back);
        assert_eq!(artifact.len(), 1);
        assert!(!artifact.is_empty());
    }

    #[test]
    fn artifact_serialises_omitting_optional_source_id() {
        let artifact = Artifact {
            rows: vec![],
            source_kind: "mysql".to_string(),
            source_id: None,
        };
        let s = serde_json::to_string(&artifact).unwrap();
        assert!(!s.contains("source_id"));
    }

    #[test]
    fn empty_artifact() {
        let artifact = Artifact {
            rows: vec![],
            source_kind: "mysql".to_string(),
            source_id: None,
        };
        assert!(artifact.is_empty());
        assert_eq!(artifact.len(), 0);
    }
}
