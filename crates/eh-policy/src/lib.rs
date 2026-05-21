//! # eh-policy
//!
//! Cedar wrapper; pure-function authorization decisions on the hot path
//!
//! ## Phase 0 status
//! This crate is a stub. Concrete implementation arrives in a later phase per
//! [§20 of the architecture](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#20-phased-implementation-plan).
//! The crate exists now so the workspace compiles end-to-end.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder until concrete implementation lands.
#[doc(hidden)]
pub const __PHASE_0_PLACEHOLDER: &str = "eh-policy stub";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_exists() {
        assert!(!__PHASE_0_PLACEHOLDER.is_empty());
    }
}
