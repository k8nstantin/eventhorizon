//! Typed connector errors.
//!
//! Connectors do NOT mint their own error code space — they map their
//! backend errors onto this taxonomy. The gateway turns these into the
//! `ErrorCode` variants in `eh-protocol`.

use thiserror::Error;

/// Result alias used by the `Connector` trait.
pub type ConnectorResult<T> = Result<T, ConnectorError>;

/// Categories of connector failure the gateway needs to distinguish.
#[derive(Debug, Error)]
pub enum ConnectorError {
    /// The connector could not establish a connection to the backend.
    /// Includes auth failures, DNS / network failures, and unreachable
    /// hosts. Maps to `ErrorCode::ConnectorError` at the edge.
    #[error("connect failed: {0}")]
    Connect(String),

    /// The connector is alive but reports it is not healthy (e.g., the
    /// backend is in read-only mode, or a known-bad state). Maps to
    /// `ErrorCode::ConnectorError` at the edge.
    #[error("unhealthy: {0}")]
    Unhealthy(String),

    /// The intent could not be compiled into a query the backend accepts —
    /// usually a structural validation failure (e.g., filter references a
    /// field not declared on the entity). Maps to `ErrorCode::InvalidIntent`.
    #[error("invalid intent for connector: {0}")]
    InvalidIntent(String),

    /// The backend refused the operation at the engine layer (e.g., the
    /// configured `eh_service` grant set does not permit it). This is the
    /// §12 debugging surface working as designed — the answer is to fix
    /// the app code or extend the grant via operator-approved PR, NEVER
    /// to bypass by switching to an admin role.
    #[error("engine refusal: {0}")]
    EngineRefusal(String),

    /// Type mismatch surfaced as data crosses the connector boundary —
    /// e.g., a `Uuid` entity field bound to a non-BINARY(16)/UUID column.
    /// Per zero-trust §14 (no type conversions), this is a typed signal,
    /// not silently coerced.
    #[error("type mismatch on field `{field}`: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Logical field name.
        field: String,
        /// Type the entity declared.
        expected: String,
        /// Type the connector saw on the wire.
        actual: String,
    },

    /// A query / insert succeeded structurally but the backend returned
    /// an error condition the connector cannot recover from. Maps to
    /// `ErrorCode::ConnectorError`.
    #[error("backend error: {0}")]
    Backend(String),

    /// A required action is not supported by this connector. Should be
    /// rare — typically caught earlier by the router via the binding's
    /// `supported_actions`. If it surfaces here, the binding's supported
    /// actions and the connector's caps diverged.
    #[error("connector does not support action `{0:?}`")]
    Unsupported(eh_core::Action),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_mismatch_displays_all_parts() {
        let e = ConnectorError::TypeMismatch {
            field: "id".to_string(),
            expected: "uuid".to_string(),
            actual: "text".to_string(),
        };
        assert_eq!(
            format!("{e}"),
            "type mismatch on field `id`: expected uuid, got text"
        );
    }

    #[test]
    fn unsupported_displays_action() {
        let e = ConnectorError::Unsupported(eh_core::Action::Append);
        assert_eq!(format!("{e}"), "connector does not support action `Append`");
    }

    #[test]
    fn engine_refusal_message_round_trip() {
        let e = ConnectorError::EngineRefusal("INSERT command denied to user".to_string());
        assert!(format!("{e}").contains("INSERT command denied"));
    }
}
