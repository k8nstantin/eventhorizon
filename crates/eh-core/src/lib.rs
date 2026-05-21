//! # eh-core
//!
//! Typed lingua franca for EventHorizon. Defines the wire-shape types every
//! other crate depends on: `Intent`, `Entity`, `EntityBinding`, `Artifact`,
//! `CallerContext`, and the action/mode/type enums.
//!
//! This crate performs **no I/O** and depends only on `serde`, `serde_json`,
//! `uuid`, and `thiserror`. It is the contract.
//!
//! Reference: [architecture §4 / §9](https://github.com/k8nstantin/eventhorizon/blob/main/eventhorizon_architecture.md#4-data-plane--datafusion-backed-federation)
//! and [SCHEMA.md §3.8 / §3.9](https://github.com/k8nstantin/eventhorizon/blob/main/SCHEMA.md#38-eh_controlentities).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod artifact;
mod caller_context;
mod entity;
mod errors;
mod intent;

pub use artifact::{Artifact, ArtifactRow};
pub use caller_context::CallerContext;
pub use entity::{Entity, EntityBinding, EntityField, FieldMap, FieldType, Profile};
pub use errors::{Error, Result};
pub use intent::{Action, Intent, Mode};
