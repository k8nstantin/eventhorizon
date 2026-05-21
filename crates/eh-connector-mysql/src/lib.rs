//! # eh-connector-mysql
//!
//! MySQL connector implementing the `Connector` trait. Both read (SELECT)
//! and write (INSERT) paths are supported by the connector itself; whether
//! a given binding exposes write is controlled by the YAML binding's
//! `supported_actions` parameter — the connector does not gate that.
//!
//! Wiring (per zero-trust §15 — public path only): the binary registers
//! this connector via `eh_connector_mysql::register(&mut registry)` behind
//! its Cargo feature flag. The kernel never references `MysqlConnector` or
//! `MysqlSourceConfig` directly.
//!
//! Wire-level discipline:
//! - Parameterised queries only — never `format!()`-built user input.
//! - UUIDv7 binds natively to `BINARY(16)` via the `sqlx`+`uuid` feature
//!   pairing. No `UUID_TO_BIN()` shim, no string round-trips (zero-trust §14).
//! - Timestamps as `chrono::NaiveDateTime`. Decimals as `rust_decimal::Decimal`.
//!   No type coercion through `String`.
//! - SCD2 columns (`valid_from`, `valid_to`, `is_current`) are set by the
//!   database defaults on INSERT — the connector NEVER references them
//!   explicitly. State changes are pure INSERTs (zero-trust §10).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod arrow_schema;
mod catalog;
mod config;
mod connector;
mod factory;
mod ident;
mod insert;
mod introspection;
mod query;
mod table_provider;
mod types;

pub use config::{MysqlSourceConfig, MysqlSslMode};
pub use connector::MysqlConnector;
pub use factory::{register, MysqlFactory, KIND};
