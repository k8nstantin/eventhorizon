//! Connector registry — the public path for adding a connector kind.
//!
//! Per zero-trust §15 (Dogfood the Application's Public Path), every
//! connector — first-party and community alike — is added the same way:
//! the gateway holds a `ConnectorRegistry` and each connector crate
//! registers a `ConnectorFactory` for its kind. No specific connector
//! type ever appears in the kernel or in `eh-bin` outside the registry
//! abstraction.
//!
//! Usage from a connector crate (e.g. `eh-connector-mysql`):
//!
//! ```text
//! pub struct MysqlFactory;
//! impl ConnectorFactory for MysqlFactory { … }
//!
//! /// Add MySQL to a `ConnectorRegistry`. The binary calls this once at
//! /// startup behind its Cargo feature gate.
//! pub fn register(registry: &mut ConnectorRegistry) {
//!     registry.register(Arc::new(MysqlFactory));
//! }
//! ```
//!
//! Usage from the binary (`eh-bin`):
//!
//! ```text
//! let mut registry = ConnectorRegistry::new();
//! #[cfg(feature = "connector-mysql")]
//! eh_connector_mysql::register(&mut registry);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_yaml::Mapping;

use crate::connector::Connector;
use crate::errors::{ConnectorError, ConnectorResult};

/// Factory for building a connector instance from an opaque YAML config
/// subtree. The factory owns the kind-string and the deserialisation of
/// the connector-specific config.
#[async_trait]
pub trait ConnectorFactory: Send + Sync + 'static {
    /// Stable kind identifier matching `eh_control.sources.kind`. The
    /// registry indexes factories by this string.
    fn kind(&self) -> &'static str;

    /// Build a fully-initialised, ready-to-use connector instance from the
    /// opaque YAML mapping that `eh-config` captured for this source.
    ///
    /// Implementations:
    /// - Deserialise the `Mapping` into their typed config struct (via
    ///   `serde_yaml::from_value(Value::Mapping(raw))`).
    /// - Resolve any secret refs (using `SecretRef::resolve`).
    /// - Open the pool / client / vector store / etc.
    /// - Return `Arc<dyn Connector>` so the gateway can share the instance
    ///   across tokio worker tasks.
    async fn build(&self, source_name: &str, raw: Mapping) -> ConnectorResult<Arc<dyn Connector>>;
}

/// The registry the gateway holds; connector crates register into it at
/// startup.
#[derive(Default)]
pub struct ConnectorRegistry {
    by_kind: HashMap<String, Arc<dyn ConnectorFactory>>,
}

impl ConnectorRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory. Refuses to overwrite an existing registration
    /// with a typed error — adding the same connector kind twice is a
    /// configuration bug the operator should know about.
    pub fn register(&mut self, factory: Arc<dyn ConnectorFactory>) -> ConnectorResult<()> {
        let kind = factory.kind().to_string();
        if self.by_kind.contains_key(&kind) {
            return Err(ConnectorError::Connect(format!(
                "connector kind {kind:?} is already registered"
            )));
        }
        self.by_kind.insert(kind, factory);
        Ok(())
    }

    /// Lookup a factory by kind. Returns `None` if no connector registered
    /// for that kind — the caller surfaces a typed error pointing at the
    /// Cargo feature that should have included the connector.
    #[must_use]
    pub fn factory(&self, kind: &str) -> Option<&Arc<dyn ConnectorFactory>> {
        self.by_kind.get(kind)
    }

    /// All registered kinds, lexically sorted. Used by error messages and
    /// `eh ctl source kinds` (which lists what's available in this binary).
    #[must_use]
    pub fn known_kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self.by_kind.keys().cloned().collect();
        kinds.sort();
        kinds
    }

    /// Build a connector by looking up the kind and delegating to its
    /// factory. Returns `ConnectorError::Connect` with a helpful message
    /// when no factory is registered for that kind.
    pub async fn build(
        &self,
        source_name: &str,
        kind: &str,
        raw: Mapping,
    ) -> ConnectorResult<Arc<dyn Connector>> {
        let factory = self.factory(kind).ok_or_else(|| {
            ConnectorError::Connect(format!(
                "no connector registered for kind {kind:?}. Known kinds: {:?}. \
                 If you intended to include this connector, enable the corresponding \
                 Cargo feature (e.g. `connector-{kind}`).",
                self.known_kinds()
            ))
        })?;
        factory.build(source_name, raw).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use datafusion::catalog::{CatalogProvider, MemoryCatalogProvider};
    use eh_core::{
        Action, Artifact, CallerContext, Entity, EntityBinding, Intent, SourceAccessScope,
    };

    use crate::caps::{ConnectorCaps, PushdownLevel};
    use crate::connector::Connector;
    use crate::errors::ConnectorError;
    use crate::outcome::AppendOutcome;

    struct DummyConnector(&'static str);

    #[async_trait]
    impl Connector for DummyConnector {
        fn kind(&self) -> &'static str {
            self.0
        }
        fn capabilities(&self) -> ConnectorCaps {
            ConnectorCaps {
                supports_read: true,
                supports_append: false,
                predicate_pushdown: PushdownLevel::None,
                projection_pushdown: false,
                streaming: false,
            }
        }
        async fn health(&self) -> ConnectorResult<()> {
            Ok(())
        }
        async fn execute_read(
            &self,
            _binding: &EntityBinding,
            _entity: &Entity,
            _intent: &Intent,
            _ctx: &CallerContext,
        ) -> ConnectorResult<Artifact> {
            Ok(Artifact {
                rows: vec![],
                source_kind: self.0.to_string(),
                source_id: None,
            })
        }
        async fn execute_append(
            &self,
            _binding: &EntityBinding,
            _entity: &Entity,
            _intent: &Intent,
            _ctx: &CallerContext,
        ) -> ConnectorResult<AppendOutcome> {
            Err(ConnectorError::Unsupported(Action::Append))
        }
        async fn build_catalog(
            &self,
            _scope: &SourceAccessScope,
        ) -> ConnectorResult<Arc<dyn CatalogProvider>> {
            // Dummy catalog with no schemas — registry tests don't
            // exercise the data path.
            Ok(Arc::new(MemoryCatalogProvider::new()))
        }
    }

    struct DummyFactory(&'static str);

    #[async_trait]
    impl ConnectorFactory for DummyFactory {
        fn kind(&self) -> &'static str {
            self.0
        }
        async fn build(
            &self,
            _source_name: &str,
            _raw: Mapping,
        ) -> ConnectorResult<Arc<dyn Connector>> {
            Ok(Arc::new(DummyConnector(self.0)))
        }
    }

    #[test]
    fn empty_registry_has_no_kinds() {
        let r = ConnectorRegistry::new();
        assert!(r.known_kinds().is_empty());
        assert!(r.factory("mysql").is_none());
    }

    #[test]
    fn register_and_lookup_by_kind() {
        let mut r = ConnectorRegistry::new();
        r.register(Arc::new(DummyFactory("dummy"))).unwrap();
        assert_eq!(r.known_kinds(), vec!["dummy".to_string()]);
        assert!(r.factory("dummy").is_some());
        assert!(r.factory("missing").is_none());
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let mut r = ConnectorRegistry::new();
        r.register(Arc::new(DummyFactory("dummy"))).unwrap();
        let err = r.register(Arc::new(DummyFactory("dummy"))).unwrap_err();
        match err {
            ConnectorError::Connect(msg) => assert!(msg.contains("already registered")),
            other => panic!("expected Connect error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn build_dispatches_to_factory() {
        let mut r = ConnectorRegistry::new();
        r.register(Arc::new(DummyFactory("dummy"))).unwrap();
        match r.build("src1", "dummy", Mapping::new()).await {
            Ok(c) => assert_eq!(c.kind(), "dummy"),
            Err(e) => panic!("build should have succeeded, got {e}"),
        }
    }

    #[tokio::test]
    async fn build_for_unregistered_kind_explains_feature_flag() {
        let r = ConnectorRegistry::new();
        // `unwrap_err` would require Debug on Arc<dyn Connector>; match instead.
        match r.build("src1", "mysql", Mapping::new()).await {
            Ok(_) => panic!("build for unregistered kind should have failed"),
            Err(ConnectorError::Connect(msg)) => {
                assert!(msg.contains("no connector registered for kind \"mysql\""));
                assert!(msg.contains("connector-mysql"));
            }
            Err(other) => panic!("expected Connect error, got {other:?}"),
        }
    }
}
