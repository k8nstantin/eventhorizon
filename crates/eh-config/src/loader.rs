//! Filesystem and env-var entrypoints for loading a config.
//!
//! Two callable forms:
//!
//! ```text
//! load_from_path(PathBuf)           // explicit --config <PATH> on the CLI
//! load_from_env()                    // reads EH_CONFIG from process env
//! ```
//!
//! Both return a fully-validated `CompiledConfig` ready for the router.
//! Secret resolution (`${ENV:NAME}` → process env) happens via the
//! `SecretRef::resolve` API; the loader itself does NOT resolve secrets
//! eagerly because some refs may legitimately be unused for the current
//! deployment (e.g., per-kind credentials when only one connector kind is
//! active). Each connector resolves its own secrets at `connect()` time
//! and refuses to start if any of *its* refs are missing.

use std::path::{Path, PathBuf};

use crate::compiled::CompiledConfig;
use crate::config::Config;
use crate::errors::{ConfigError, ConfigResult};

/// Default config-file env var name.
pub const EH_CONFIG_ENV: &str = "EH_CONFIG";

/// Load and compile a config from the given path.
pub fn load_from_path<P: AsRef<Path>>(path: P) -> ConfigResult<CompiledConfig> {
    let path_ref = path.as_ref();
    let text = std::fs::read_to_string(path_ref).map_err(|source| ConfigError::Io {
        path: path_ref.to_path_buf(),
        source,
    })?;
    let cfg: Config = serde_yaml::from_str(&text)?;
    cfg.compile()
}

/// Load and compile a config from the path pointed to by `EH_CONFIG`.
pub fn load_from_env() -> ConfigResult<CompiledConfig> {
    let path = std::env::var(EH_CONFIG_ENV)
        .map(PathBuf::from)
        .map_err(|_| ConfigError::NoConfigPath)?;
    load_from_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp_yaml(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn load_from_path_compiles_fvp_yaml() {
        let yaml = r#"
version: 1
sources:
  fvp_mysql:
    kind: mysql
    host: mysql
    port: 3306
    database: eh_demo
    username: eh_service
    password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
entities:
  Customer:
    fields:
      - { name: id,    data_type: uuid,   nullable: false, pii: false }
      - { name: email, data_type: string, nullable: false, pii: true  }
bindings:
  - entity: Customer
    source: fvp_mysql
    physical_table: eh_demo.customers
    profile: oltp
    supported_actions: [read]
    field_map:
      id: id
      email: email
routing:
  - { when: { entity: Customer, action: read }, target: fvp_mysql }
"#;
        let f = write_tmp_yaml(yaml);
        let compiled = load_from_path(f.path()).unwrap();
        assert!(compiled.entity("Customer").is_some());
        assert!(compiled.source("fvp_mysql").is_some());
        assert_eq!(compiled.bindings_for_entity("Customer").len(), 1);
        assert_eq!(compiled.routing.len(), 1);
    }

    #[test]
    fn load_from_path_missing_file() {
        let err = load_from_path("/__definitely_not_a_real_path__.yaml").unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    #[test]
    #[allow(unsafe_code)]
    fn load_from_env_without_var_fails_with_no_config_path() {
        // SAFETY: removing the EH_CONFIG env var; this test must not run
        // concurrently with anything that reads EH_CONFIG. Production code
        // does not call env::remove_var.
        unsafe { std::env::remove_var(EH_CONFIG_ENV) };
        let err = load_from_env().unwrap_err();
        assert!(matches!(err, ConfigError::NoConfigPath));
    }
}
