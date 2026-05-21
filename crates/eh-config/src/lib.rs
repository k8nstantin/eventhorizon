//! # eh-config
//!
//! YAML configuration loader for the FVP control plane. Reads a file
//! describing sources, entities, bindings, and routing rules, resolves
//! `${ENV:NAME}` secret references against process environment variables,
//! validates the structure, and yields a `CompiledConfig` ready for the
//! router and compiler.
//!
//! Phase 6+ replaces this loader's RUNTIME state with `eh-control-pg` (the
//! durable Postgres-backed control plane). The YAML loader stays for tests
//! and offline tooling.
//!
//! Reference: [architecture §5.9 / §6](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#6-edge-protocols--mcp-rest-grpc)
//! and [SCHEMA.md §6](https://github.com/k8nstantin/eventhorizon/blob/main/SCHEMA.md#6-phase-1-worked-example--tenant-customers-table-on-mysql).

// `deny` (not `forbid`) so the env-var resolver tests can scope an explicit
// `#[allow(unsafe_code)]` around the `env::set_var` / `env::remove_var` calls
// — those are marked `unsafe` since Rust 1.84 because they are not thread-
// safe. Production code does not use them.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod cache;
mod compiled;
mod config;
mod errors;
mod loader;
mod routing;
mod secret;
mod source;

pub use cache::ConfigCache;
pub use compiled::CompiledConfig;
pub use config::Config;
pub use errors::{ConfigError, ConfigResult};
pub use loader::{load_from_env, load_from_path};
pub use routing::{RoutingMatch, RoutingRule};
pub use secret::{Secret, SecretRef};
pub use source::SourceConfig;
