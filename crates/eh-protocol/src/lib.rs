//! # eh-protocol
//!
//! Wire-format envelopes for the REST and MCP edges. Defines what the
//! gateway accepts and returns at the public surface, and the structured
//! error shape every edge translates engine refusals / config errors into.
//!
//! The transport-agnostic shape — `IntentEnvelope` in, `ResponseEnvelope`
//! out — keeps the REST and MCP edges identical from the gateway's point
//! of view; only the framing (HTTP body vs MCP tool-call result) differs.
//!
//! Reference: [architecture §6 / §9](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#6-edge-protocols--mcp-rest-grpc).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod envelope;
mod error_response;

pub use envelope::{IntentEnvelope, ResponseEnvelope};
pub use error_response::{ErrorCode, ErrorResponse};
