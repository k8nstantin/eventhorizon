//! `MysqlFactory` — the public registration path for the MySQL connector.
//!
//! Per zero-trust §15, the kernel never references `MysqlConnector`
//! directly. Instead, this factory is added to the gateway's
//! `ConnectorRegistry` at startup via the `register` function below; the
//! registry hands the factory the opaque YAML mapping for each
//! `kind: mysql` source it encounters, and the factory deserialises it
//! into `MysqlSourceConfig` + opens the pool.
//!
//! Community connector authors use exactly this pattern.

use std::sync::Arc;

use async_trait::async_trait;
use eh_connector_api::{
    Connector, ConnectorError, ConnectorFactory, ConnectorRegistry, ConnectorResult,
};
use serde_yaml::{Mapping, Value};
use tracing::{info, instrument};

use crate::config::MysqlSourceConfig;
use crate::connector::MysqlConnector;

/// MySQL connector kind identifier — matches `eh_control.sources.kind`.
pub const KIND: &str = "mysql";

/// Factory that builds `MysqlConnector` instances from an opaque YAML
/// mapping.
pub struct MysqlFactory;

#[async_trait]
impl ConnectorFactory for MysqlFactory {
    fn kind(&self) -> &'static str {
        KIND
    }

    #[instrument(skip(self, raw), fields(kind = KIND, source = %source_name))]
    async fn build(&self, source_name: &str, raw: Mapping) -> ConnectorResult<Arc<dyn Connector>> {
        let cfg: MysqlSourceConfig = serde_yaml::from_value(Value::Mapping(raw)).map_err(|e| {
            ConnectorError::Connect(format!(
                "invalid mysql config for source {source_name:?}: {e}"
            ))
        })?;
        let connector = MysqlConnector::connect(&cfg).await?;
        info!(target: "eh.connector.mysql", "connector ready");
        Ok(Arc::new(connector))
    }
}

/// Public registration entrypoint. The binary calls this once at startup
/// when the `connector-mysql` Cargo feature is enabled:
///
/// ```text
/// let mut registry = ConnectorRegistry::new();
/// #[cfg(feature = "connector-mysql")]
/// eh_connector_mysql::register(&mut registry)?;
/// ```
///
/// Refuses to overwrite an existing registration (the registry surfaces
/// the typed error).
pub fn register(registry: &mut ConnectorRegistry) -> ConnectorResult<()> {
    registry.register(Arc::new(MysqlFactory))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_is_mysql() {
        assert_eq!(MysqlFactory.kind(), "mysql");
        assert_eq!(KIND, "mysql");
    }

    #[test]
    fn register_adds_to_empty_registry() {
        let mut r = ConnectorRegistry::new();
        register(&mut r).unwrap();
        assert_eq!(r.known_kinds(), vec!["mysql".to_string()]);
        assert!(r.factory("mysql").is_some());
    }

    #[test]
    fn duplicate_register_is_rejected() {
        let mut r = ConnectorRegistry::new();
        register(&mut r).unwrap();
        let err = register(&mut r).unwrap_err();
        assert!(matches!(err, ConnectorError::Connect(_)));
    }

    #[tokio::test]
    async fn build_with_invalid_config_returns_typed_error() {
        let f = MysqlFactory;
        let mut raw = Mapping::new();
        // Missing required `host` and `database` keys — must surface as
        // a typed Connect error, not a panic.
        raw.insert(Value::String("port".into()), Value::Number(3306.into()));
        // `unwrap_err` needs Debug on Arc<dyn Connector>; match instead.
        match f.build("broken_source", raw).await {
            Ok(_) => panic!("build with invalid config should have failed"),
            Err(ConnectorError::Connect(msg)) => {
                assert!(msg.contains("broken_source"));
                assert!(msg.contains("mysql config"));
            }
            Err(other) => panic!("expected Connect error, got {other:?}"),
        }
    }
}
