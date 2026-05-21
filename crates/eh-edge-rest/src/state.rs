//! Shared application state mounted into the axum router.
//!
//! The state holds:
//!
//! * The `ConfigCache` (lock-free `Arc<ArcSwap<CompiledConfig>>`).
//! * A `ConnectorMap`: source-name → `Arc<dyn Connector>`. The map is
//!   populated by `eh-bin` at startup via `ConnectorRegistry::build` —
//!   the public path, never the kernel reaching directly into a specific
//!   connector type.
//! * A default tenant id used to populate `CallerContext.tenant_id` for
//!   Phase 1 single-tenant deployments. Phase 6+ pulls this from the
//!   agent's record in the control plane.

use std::collections::HashMap;
use std::sync::Arc;

use eh_config::ConfigCache;
use eh_connector_api::Connector;
use uuid::Uuid;

/// source name → connector instance.
pub type ConnectorMap = HashMap<String, Arc<dyn Connector>>;

/// Application state passed to every handler.
pub struct AppState {
    /// Loaded config cache.
    pub config: ConfigCache,
    /// Connectors built from the loaded source configs.
    pub connectors: ConnectorMap,
    /// Phase 1 default tenant id. The single-tenant FVP runs all
    /// requests against this id; multi-tenant routing lands in Phase 6.
    pub default_tenant: Uuid,
}

impl AppState {
    /// Construct from already-built pieces.
    #[must_use]
    pub fn new(config: ConfigCache, connectors: ConnectorMap, default_tenant: Uuid) -> Self {
        Self {
            config,
            connectors,
            default_tenant,
        }
    }

    /// Look up the connector for a source name.
    #[must_use]
    pub fn connector_for(&self, source_name: &str) -> Option<&Arc<dyn Connector>> {
        self.connectors.get(source_name)
    }
}
