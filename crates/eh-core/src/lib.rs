//! # eh-core
//!
//! Typed lingua franca for EventHorizon. Defines `Intent`, `Entity`, `Binding`,
//! `Artifact`, `CallerContext`, and the lifecycle enums (`Action`, `Mode`).
//!
//! This crate performs **no I/O** and depends only on `serde`, `uuid`, and
//! error/thiserror plumbing. Every other crate in the workspace consumes
//! these types as the contract.
//!
//! See [eventhorizon_architecture.md §3](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#3-the-kernel--module-system) for the
//! kernel's role and module boundaries.
//!
//! ## Phase 0 status
//! This crate is a stub. Concrete types arrive in Phase 1 (FVP). The crate
//! exists now so the workspace compiles end-to-end.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder until Phase 1 lands the real types.
#[doc(hidden)]
pub const __PHASE_0_PLACEHOLDER: &str = "eh-core stub — types arrive Phase 1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_exists() {
        assert!(!__PHASE_0_PLACEHOLDER.is_empty());
    }
}
