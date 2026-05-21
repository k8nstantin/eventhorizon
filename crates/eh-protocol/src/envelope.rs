//! The wire-format envelopes carried by the REST and MCP edges.
//!
//! `IntentEnvelope` is the request shape (intent body + bearer token);
//! `ResponseEnvelope` is the response (`success` or `error`, mutually
//! exclusive). Tagged externally so the response is unambiguous to parse.

use eh_core::{Artifact, Intent};
use serde::{Deserialize, Serialize};

use crate::error_response::ErrorResponse;

/// Wire-format request body.
///
/// The agent bearer token rides alongside the intent so the edge can
/// authenticate without out-of-band headers when needed (e.g., in MCP tool
/// calls where authentication is conveyed in the tool-call arguments).
/// In REST deployments the token may also arrive via an `Authorization`
/// header — the edge accepts either form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentEnvelope {
    /// The bearer token identifying the calling agent.
    pub agent_token: String,
    /// The intent body.
    pub intent: Intent,
}

/// Wire-format response body.
///
/// Externally-tagged enum with two variants. JSON parsers can branch on the
/// `result` tag.
///
/// ```json
/// { "result": "success", "data": { "rows": [...], "source_kind": "mysql" } }
/// { "result": "error",   "data": { "code": "unknown_entity", "message": "..." } }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "data", rename_all = "snake_case")]
pub enum ResponseEnvelope {
    /// Intent executed; the artifact carries the rows.
    Success(Artifact),
    /// Intent rejected (authz / validation / engine refusal / connector error).
    Error(ErrorResponse),
}

impl ResponseEnvelope {
    /// True if this is a success response.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, ResponseEnvelope::Success(_))
    }

    /// True if this is an error response.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, ResponseEnvelope::Error(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_response::ErrorCode;
    use eh_core::{Action, ArtifactRow};
    use serde_json::json;

    fn sample_intent() -> Intent {
        Intent {
            action: Action::Read,
            entity: "Customer".to_string(),
            mode: None,
            fields: Some(vec!["id".to_string(), "email".to_string()]),
            filter: Some(json!({ "id": "cust_1" })),
            payload: None,
        }
    }

    #[test]
    fn intent_envelope_round_trip() {
        let env = IntentEnvelope {
            agent_token: "bearer-xyz".to_string(),
            intent: sample_intent(),
        };
        let json = serde_json::to_value(&env).unwrap();
        let back: IntentEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn intent_envelope_wire_shape() {
        let env = IntentEnvelope {
            agent_token: "tok".to_string(),
            intent: Intent {
                action: Action::Read,
                entity: "Customer".to_string(),
                mode: None,
                fields: None,
                filter: None,
                payload: None,
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        assert_eq!(
            s,
            r#"{"agent_token":"tok","intent":{"action":"read","entity":"Customer"}}"#
        );
    }

    #[test]
    fn success_response_round_trip() {
        let mut row = ArtifactRow::new();
        row.insert("id", json!("cust_1"));
        let artifact = Artifact {
            rows: vec![row],
            source_kind: "mysql".to_string(),
            source_id: None,
        };
        let env = ResponseEnvelope::Success(artifact);
        let json = serde_json::to_value(&env).unwrap();
        let back: ResponseEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(env, back);
        assert!(back.is_success());
        assert!(!back.is_error());
    }

    #[test]
    fn error_response_round_trip() {
        let env = ResponseEnvelope::Error(ErrorResponse::new(
            ErrorCode::UnknownEntity,
            "no entity named 'Frobnicator'",
        ));
        let json = serde_json::to_value(&env).unwrap();
        let back: ResponseEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(env, back);
        assert!(back.is_error());
        assert!(!back.is_success());
    }

    #[test]
    fn success_response_wire_shape() {
        let env = ResponseEnvelope::Success(Artifact {
            rows: vec![],
            source_kind: "mysql".to_string(),
            source_id: None,
        });
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(
            v,
            json!({
                "result": "success",
                "data": { "rows": [], "source_kind": "mysql" }
            })
        );
    }

    #[test]
    fn error_response_wire_shape() {
        let env = ResponseEnvelope::Error(ErrorResponse::new(
            ErrorCode::Unauthorized,
            "missing or invalid agent_token",
        ));
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(
            v,
            json!({
                "result": "error",
                "data": {
                    "code": "unauthorized",
                    "message": "missing or invalid agent_token"
                }
            })
        );
    }
}
