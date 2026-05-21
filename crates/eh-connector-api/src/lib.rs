//! # eh-connector-api
//!
//! Public trait + registry every EventHorizon backend implements / plugs
//! into. Defines the capability surface, the typed connector error
//! taxonomy, the read / append execution contract, and the
//! `ConnectorRegistry` through which connectors are added to the running
//! gateway — the same path a community connector author would use
//! (zero-trust §15).
//!
//! The Phase 1 surface intentionally exposes only SELECT-shaped reads and
//! INSERT-shaped appends. UPDATE / DELETE / DDL are not part of the
//! contract — they are not actions the application code is allowed to
//! perform per zero-trust §10.
//!
//! Reference: [architecture §9](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#9-connector-trait--lifecycle)
//! and [CONNECTORS.md](https://github.com/k8nstantin/eventhorizon/blob/main/CONNECTORS.md).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod caps;
mod connector;
mod errors;
mod outcome;
mod registry;

pub use caps::{ConnectorCaps, PushdownLevel};
pub use connector::Connector;
pub use errors::{ConnectorError, ConnectorResult};
pub use outcome::AppendOutcome;
pub use registry::{ConnectorFactory, ConnectorRegistry};
