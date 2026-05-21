//! # eh-control-api
//!
//! ControlPlane trait (agents, entities, bindings, rules) — backend-agnostic
//!
//! ## Phase 0 status
//! This crate is a stub. Concrete implementation arrives in a later phase per
//! [§20 of the architecture](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#20-phased-implementation-plan).
//! The crate exists now so the workspace compiles end-to-end.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder until concrete implementation lands.
#[doc(hidden)]
pub const __PHASE_0_PLACEHOLDER: &str = "eh-control-api stub";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_exists() {
        assert!(!__PHASE_0_PLACEHOLDER.is_empty());
    }
}
