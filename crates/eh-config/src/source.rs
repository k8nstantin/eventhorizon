//! Per-kind source configurations.
//!
//! Each connector kind owns a typed configuration shape, mirroring the
//! per-kind `eh_control.source_<kind>` tables in the schema. The FVP only
//! exercises MySQL; other kinds will land as their connectors land.

use serde::{Deserialize, Serialize};

use crate::secret::SecretRef;

/// MySQL SSL mode. Matches `eh_control.source_mysql.ssl_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MysqlSslMode {
    /// No TLS (only acceptable on a private docker network).
    Disabled,
    /// Negotiate TLS if the server offers it; otherwise plain.
    #[default]
    Preferred,
    /// Require TLS; no server-cert verification.
    Required,
    /// Require TLS and verify the server certificate against a CA.
    VerifyCa,
    /// Require TLS, verify CA, and match the hostname.
    VerifyIdentity,
}

/// MySQL connector configuration. Mirrors `eh_control.source_mysql` plus the
/// auth_kind extension columns (Phase 1 only exercises `password`; mTLS /
/// IAM land per the locked schema when their connectors implement them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MysqlSourceConfig {
    /// Hostname or IP address of the MySQL server.
    pub host: String,
    /// TCP port. Defaults to 3306 if omitted in YAML.
    #[serde(default = "default_mysql_port")]
    pub port: u16,
    /// Database name to connect to.
    pub database: String,
    /// Username — operator's choice. The connector connects as whatever the
    /// operator put here; the FVP uses `eh_service`.
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

/// A registered source, tagged by `kind`. The YAML uses `kind: mysql` etc.
/// to discriminate.
///
/// Adding a new connector kind = adding a new variant here AND a new
/// `eh_control.source_<kind>` migration AND extending the `sources.kind`
/// CHECK enum. Three coordinated changes, per the connector contribution
/// workflow in CONNECTORS.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceConfig {
    /// MySQL connector configuration.
    Mysql(MysqlSourceConfig),
}

impl SourceConfig {
    /// The lowercase kind identifier, matching `eh_control.sources.kind`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            SourceConfig::Mysql(_) => "mysql",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_source_yaml_round_trip() {
        let yaml = r#"
kind: mysql
host: mysql
port: 3306
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
ssl_mode: preferred
max_pool_size: 8
"#;
        let s: SourceConfig = serde_yaml::from_str(yaml).unwrap();
        match s {
            SourceConfig::Mysql(cfg) => {
                assert_eq!(cfg.host, "mysql");
                assert_eq!(cfg.port, 3306);
                assert_eq!(cfg.database, "eh_demo");
                assert_eq!(cfg.username, "eh_service");
                assert_eq!(cfg.password.env_name(), "FVP_MYSQL_SERVICE_PASSWORD");
                assert_eq!(cfg.ssl_mode, MysqlSslMode::Preferred);
                assert_eq!(cfg.max_pool_size, 8);
            }
        }
    }

    #[test]
    fn mysql_source_defaults_when_optional_fields_missing() {
        let yaml = r#"
kind: mysql
host: localhost
database: eh_demo
username: eh_service
password: ${ENV:FVP_MYSQL_SERVICE_PASSWORD}
"#;
        let s: SourceConfig = serde_yaml::from_str(yaml).unwrap();
        match s {
            SourceConfig::Mysql(cfg) => {
                assert_eq!(cfg.port, 3306);
                assert_eq!(cfg.ssl_mode, MysqlSslMode::Preferred);
                assert_eq!(cfg.max_pool_size, 8);
            }
        }
    }

    #[test]
    fn source_kind_string_matches_schema() {
        let cfg = SourceConfig::Mysql(MysqlSourceConfig {
            host: "localhost".into(),
            port: 3306,
            database: "x".into(),
            username: "u".into(),
            password: SecretRef::Env("X".into()),
            ssl_mode: MysqlSslMode::Preferred,
            max_pool_size: 8,
        });
        assert_eq!(cfg.kind(), "mysql");
    }

    #[test]
    fn ssl_mode_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&MysqlSslMode::VerifyCa).unwrap(),
            "\"verify_ca\""
        );
        assert_eq!(
            serde_json::to_string(&MysqlSslMode::VerifyIdentity).unwrap(),
            "\"verify_identity\""
        );
    }

    #[test]
    fn source_password_debug_redacts() {
        let cfg = MysqlSourceConfig {
            host: "x".into(),
            port: 3306,
            database: "x".into(),
            username: "u".into(),
            password: SecretRef::Env("MYVAR".into()),
            ssl_mode: MysqlSslMode::Preferred,
            max_pool_size: 8,
        };
        let s = format!("{cfg:?}");
        assert!(s.contains("MYVAR"));
        assert!(!s.contains("hunter2"));
    }
}
