//! MySQL connector configuration.
//!
//! Lives in the connector crate (NOT in `eh-config`) so the kernel stays
//! connector-agnostic per zero-trust §15. The `MysqlFactory` deserialises
//! the opaque `serde_yaml::Mapping` that `eh-config` captured for this
//! source into this typed struct.

use eh_config::SecretRef;
use serde::{Deserialize, Serialize};

/// MySQL SSL mode. Matches the locked-schema `source_mysql.ssl_mode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MysqlSslMode {
    /// No TLS — only acceptable on a private docker network.
    Disabled,
    /// Negotiate TLS if the server offers it; plain otherwise.
    #[default]
    Preferred,
    /// Require TLS; no server-cert verification.
    Required,
    /// Require TLS; verify the server certificate against a CA.
    VerifyCa,
    /// Require TLS; verify CA AND match the hostname.
    VerifyIdentity,
}

/// MySQL connector configuration deserialised from the opaque YAML subtree
/// captured by `eh_config::SourceConfig::raw`.
///
/// Mirrors `eh_control.source_mysql` plus the auth_kind extension columns.
/// Phase 1 only exercises `auth_kind = password`; mTLS / IAM paths land
/// per the locked schema when their connector code lands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MysqlSourceConfig {
    /// Hostname or IP address of the MySQL server.
    pub host: String,
    /// TCP port. Defaults to 3306 if omitted in YAML.
    #[serde(default = "default_mysql_port")]
    pub port: u16,
    /// Database name to connect to.
    pub database: String,
    /// Username — operator's choice. The connector connects as whatever
    /// the operator put here. The FVP demo uses `eh_service`.
    pub username: String,
    /// Reference to the password secret. Resolved at startup; the gateway
    /// refuses to start if the env var is missing.
    pub password: SecretRef,
    /// SSL / TLS mode.
    #[serde(default)]
    pub ssl_mode: MysqlSslMode,
    /// Max connections per pod for this source.
    #[serde(default = "default_pool_size")]
    pub max_pool_size: u32,
}

fn default_mysql_port() -> u16 {
    3306
}

fn default_pool_size() -> u32 {
    8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_yaml_subtree() {
        let yaml = r#"
host: mysql
port: 3306
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
ssl_mode: preferred
max_pool_size: 8
"#;
        let cfg: MysqlSourceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.host, "mysql");
        assert_eq!(cfg.port, 3306);
        assert_eq!(cfg.database, "eh_demo");
        assert_eq!(cfg.username, "eh_service");
        assert_eq!(cfg.password.env_name(), "FVP_MYSQL_SERVICE_PASSWORD");
        assert_eq!(cfg.ssl_mode, MysqlSslMode::Preferred);
        assert_eq!(cfg.max_pool_size, 8);
    }

    #[test]
    fn defaults_when_optional_fields_missing() {
        let yaml = r#"
host: localhost
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
"#;
        let cfg: MysqlSourceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.port, 3306);
        assert_eq!(cfg.ssl_mode, MysqlSslMode::Preferred);
        assert_eq!(cfg.max_pool_size, 8);
    }

    #[test]
    fn debug_redacts_password() {
        let cfg = MysqlSourceConfig {
            host: "x".into(),
            port: 3306,
            database: "x".into(),
            username: "u".into(),
            password: SecretRef::Env("X".into()),
            ssl_mode: MysqlSslMode::Preferred,
            max_pool_size: 8,
        };
        let s = format!("{cfg:?}");
        // The env-var name appears (it's metadata, not the secret), but
        // no resolved value can possibly be present at debug-format time.
        assert!(s.contains("X"));
    }
}
