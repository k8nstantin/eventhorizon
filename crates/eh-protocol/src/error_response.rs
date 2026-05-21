//! Structured wire-format errors.
//!
//! Every error the edge surfaces to a caller is one of a small, named set
//! of `ErrorCode` values. The named set means client code can branch on
//! the code without parsing the human-readable `message`.

use serde::{Deserialize, Serialize};

/// Categories of error the gateway can surface to a caller.
///
/// New codes land per architectural change, not per failure. Connectors do
/// NOT mint new codes — connector-specific failures map onto one of these
/// (typically `ConnectorError` for unexpected backend errors or
/// `OverBudget` / `CircuitOpen` for cost / breaker rejections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Missing or invalid agent credentials.
    Unauthorized,

    /// Agent authenticated but lacks the capability for this intent.
    Forbidden,

    /// Intent body fails JSON / type validation (e.g., unknown field type,
    /// payload missing for `Append`).
    InvalidIntent,

    /// Intent referenced an entity not declared in the loaded configuration.
    UnknownEntity,

    /// No binding satisfies the routing rule for this intent.
    NoBinding,

    /// Plan estimated cost / scan size exceeds the agent or source budget
    /// (Phase 9+).
    OverBudget,

    /// The source's circuit breaker is open (Phase 9+).
    CircuitOpen,

    /// Per-agent rate limit hit; client should retry after `retry_after_ms`.
    RateLimited,

    /// Unexpected failure inside a connector. Treated as a server error;
    /// the gateway logs the connector's typed error internally.
    ConnectorError,

    /// Server-side configuration error (e.g., source declared in config but
    /// no connector registered for that kind).
    ConfigError,

    /// Catch-all for unmapped failures. Should be rare; presence indicates
    /// a missing taxonomy entry to be addressed in a follow-up PR.
    Internal,
}

/// Wire-format error body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Stable code clients can branch on.
    pub code: ErrorCode,
    /// Human-readable message for logs / debug. Not stable; do not parse.
    pub message: String,
    /// Suggested back-off for `RateLimited` / `CircuitOpen`. Milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl ErrorResponse {
    /// Construct an error response with a code and message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    /// Construct an error response with a code, message, and retry hint.
    #[must_use]
    pub fn with_retry(code: ErrorCode, message: impl Into<String>, retry_after_ms: u64) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::UnknownEntity).unwrap(),
            "\"unknown_entity\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::OverBudget).unwrap(),
            "\"over_budget\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::ConnectorError).unwrap(),
            "\"connector_error\""
        );
    }

    #[test]
    fn error_response_round_trip() {
        let e = ErrorResponse::new(ErrorCode::InvalidIntent, "missing required field 'entity'");
        let json = serde_json::to_value(&e).unwrap();
        let back: ErrorResponse = serde_json::from_value(json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn error_response_with_retry_round_trip() {
        let e = ErrorResponse::with_retry(ErrorCode::RateLimited, "slow down", 1500);
        let json = serde_json::to_value(&e).unwrap();
        let back: ErrorResponse = serde_json::from_value(json).unwrap();
        assert_eq!(e, back);
        assert_eq!(back.retry_after_ms, Some(1500));
    }

    #[test]
    fn error_response_omits_retry_when_none() {
        let e = ErrorResponse::new(ErrorCode::Unauthorized, "bad token");
        let s = serde_json::to_string(&e).unwrap();
        assert!(!s.contains("retry_after_ms"));
    }

    #[test]
    fn error_response_includes_retry_when_set() {
        let e = ErrorResponse::with_retry(ErrorCode::CircuitOpen, "source paused", 5000);
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("retry_after_ms"));
        assert!(s.contains("5000"));
    }
}
