//! Config loader / validation errors.

use std::path::PathBuf;

use thiserror::Error;

/// Result alias used throughout the crate.
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Typed errors the config loader returns.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// I/O failure reading the config file from disk.
    #[error("failed to read config file {path:?}: {source}")]
    Io {
        /// Path the loader was asked to open.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// YAML parse failure.
    #[error("config YAML is invalid: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// A `${ENV:NAME}` reference is malformed.
    #[error("invalid secret reference {0:?}; expected form `${{ENV:NAME}}`")]
    InvalidSecretRef(String),

    /// A `${ENV:NAME}` reference points to an env var that is not set.
    #[error(
        "required environment variable {0:?} is not set (referenced by a config secret); \
         the gateway refuses to start with missing secrets"
    )]
    MissingEnvVar(String),

    /// A `${ENV:NAME}` reference points to an env var that is empty.
    #[error("environment variable {0:?} is set but empty")]
    EmptyEnvVar(String),

    /// `EH_CONFIG` env var is not set and no `--config` was supplied.
    #[error("EH_CONFIG is not set and no --config path was supplied")]
    NoConfigPath,

    /// A binding references an entity that is not declared.
    #[error("binding references unknown entity {0:?}")]
    UnknownEntityInBinding(String),

    /// A binding references a source that is not declared.
    #[error("binding references unknown source {0:?}")]
    UnknownSourceInBinding(String),

    /// A binding's field_map references a logical field that the entity
    /// does not declare.
    #[error("binding for entity {entity:?} references unknown field {field:?} in field_map")]
    UnknownFieldInBinding {
        /// Entity name.
        entity: String,
        /// Field name.
        field: String,
    },

    /// A routing rule's target is not a known source.
    #[error("routing rule targets unknown source {0:?}")]
    UnknownTargetInRoute(String),

    /// A routing rule's `when.entity` is not a known entity.
    #[error("routing rule's `when.entity` is unknown: {0:?}")]
    UnknownEntityInRoute(String),

    /// `version:` in the config file is not the supported version.
    #[error("unsupported config version {found}; supported: {supported}")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Version this loader supports.
        supported: u32,
    },
}
