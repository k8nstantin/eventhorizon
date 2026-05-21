//! `ConfigCache` — the lock-free per-pod cache of `CompiledConfig`.
//!
//! Phase 1 uses this as a process-local container. Phase 6 will subscribe
//! to Postgres LISTEN/NOTIFY and call `swap` on the cache when control
//! plane state changes; for Phase 1 the cache is populated once at startup
//! and never replaced (SIGHUP-driven reload lands in Phase 2 along with
//! the other deferred FVP-polish items).

use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::compiled::CompiledConfig;

/// Lock-free holder for the currently-active compiled config.
///
/// Cheap to clone (`Arc` inside); cheap to `load()` on the hot path
/// (returns a `Guard` that defers the underlying `Arc` drop).
#[derive(Debug)]
pub struct ConfigCache {
    inner: Arc<ArcSwap<CompiledConfig>>,
}

impl ConfigCache {
    /// Create a new cache pre-populated with `initial`.
    #[must_use]
    pub fn new(initial: CompiledConfig) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(initial))),
        }
    }

    /// Snapshot the currently-active config. Hot-path safe.
    #[must_use]
    pub fn load(&self) -> Arc<CompiledConfig> {
        self.inner.load_full()
    }

    /// Replace the active config atomically. Used on hot reload.
    pub fn swap(&self, next: CompiledConfig) {
        self.inner.store(Arc::new(next));
    }
}

impl Clone for ConfigCache {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    fn empty_compiled() -> CompiledConfig {
        CompiledConfig {
            sources: BTreeMap::new(),
            entities: BTreeMap::new(),
            bindings_by_entity: HashMap::new(),
            routing: vec![],
        }
    }

    #[test]
    fn new_then_load_returns_the_initial_config() {
        let cache = ConfigCache::new(empty_compiled());
        let cfg = cache.load();
        assert!(cfg.entities.is_empty());
    }

    #[test]
    fn swap_replaces_active_config_atomically() {
        let mut first = empty_compiled();
        first.entities.insert(
            "A".to_string(),
            eh_core::Entity {
                name: "A".into(),
                fields: vec![],
            },
        );
        let cache = ConfigCache::new(first);
        assert!(cache.load().entity("A").is_some());

        let mut second = empty_compiled();
        second.entities.insert(
            "B".to_string(),
            eh_core::Entity {
                name: "B".into(),
                fields: vec![],
            },
        );
        cache.swap(second);

        let after = cache.load();
        assert!(after.entity("A").is_none());
        assert!(after.entity("B").is_some());
    }

    #[test]
    fn clones_share_the_same_underlying_state() {
        let cache_a = ConfigCache::new(empty_compiled());
        let cache_b = cache_a.clone();

        let mut next = empty_compiled();
        next.entities.insert(
            "X".to_string(),
            eh_core::Entity {
                name: "X".into(),
                fields: vec![],
            },
        );
        cache_a.swap(next);

        // The clone sees the swap immediately — it's the same Arc.
        assert!(cache_b.load().entity("X").is_some());
    }
}
