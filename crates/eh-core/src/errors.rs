//! Crate-wide error taxonomy.
//!
//! `eh-core` errors are the typed kinds every downstream crate maps to. They
//! deliberately *do not* carry connector-specific details; the connector
//! reports those via its own error type and the gateway folds them into the
//! `Other` variant when surfacing.

use thiserror::Error;

/// The crate's `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Typed errors `eh-core` users return.
#[derive(Debug, Error)]
pub enum Error {
    /// Intent referenced an entity not declared in the loaded configuration.
    #[error("unknown entity: {0}")]
    UnknownEntity(String),

    /// Intent's binding referenced a source not declared in the loaded
    /// configuration.
    #[error("unknown source: {0}")]
    UnknownSource(String),

    /// The intent is well-formed JSON but violates a structural rule (e.g.,
    /// `Append` without a payload, projection of a field not declared by
    /// the entity, filter referencing an unknown field).
    #[error("invalid intent: {0}")]
    InvalidIntent(String),

    /// Configuration parse / validation failure.
    #[error("config error: {0}")]
    Config(String),

    /// Type-conversion failure surfaced as data crosses the layer boundary
    /// (e.g., a `Uuid` field bound to a non-BINARY(16)/UUID column on the
    /// backend). Per zero-trust §14, conversions must not happen silently
    /// — this is the typed signal when one is required.
    #[error("type mismatch on field `{field}`: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Logical field name.
        field: String,
        /// Type the entity declared.
        expected: String,
        /// Type the connector produced.
        actual: String,
    },

    /// Any other error, surfaced via `anyhow::Error` from a layer below.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_entity_displays_with_name() {
        let e = Error::UnknownEntity("Customer".to_string());
        assert_eq!(format!("{e}"), "unknown entity: Customer");
    }

    #[test]
    fn type_mismatch_displays_all_three_parts() {
        let e = Error::TypeMismatch {
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
    fn other_wraps_anyhow() {
        fn anyhow_caller() -> Result<()> {
            Err(anyhow::anyhow!("upstream failure"))?
        }
        let err = anyhow_caller().unwrap_err();
        assert!(matches!(err, Error::Other(_)));
        assert_eq!(format!("{err}"), "upstream failure");
    }
}
