//! `CallerContext` — per-request identity + tracing carried alongside an
//! `Intent` through the pipeline.
//!
//! The context is what authorization, identity passthrough, and telemetry
//! key on. It is constructed at the edge (REST / MCP / gRPC) from the
//! agent's authentication material and propagates through every layer.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Per-intent identity + tracing context.
///
/// Field semantics:
///
/// * `tenant_id` — every intent is tenant-scoped. RLS on the control plane
///   reads this via `current_setting('app.tenant_id')` (set by the gateway
///   per request, per architecture §5.8).
/// * `agent_id` — the agent's identifier in `eh_control.agents`. `None`
///   while Phase 1 still runs without a populated control plane.
/// * `trace_id` — UUIDv7, generated at the edge, propagated through OTel
///   spans, audit_log rows, and telemetry_events rows.
/// * `agent_token` — the bearer token used to authenticate the edge call.
///   Kept *only* for the Phase 1 FVP REST path; the moment Cedar lands in
///   Phase 5 the token is exchanged at the edge for a principal and never
///   touches downstream code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerContext {
    /// Tenant the intent runs against.
    pub tenant_id: Uuid,
    /// Agent identifier, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    /// Trace correlation id. UUIDv7.
    pub trace_id: Uuid,
    /// Phase 1 bearer token. Removed when Cedar lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_token: Option<String>,
}

impl CallerContext {
    /// Build a minimal context for tests / smoke runs. Generates a fresh
    /// UUIDv7 trace id.
    #[must_use]
    pub fn for_tenant(tenant_id: Uuid) -> Self {
        Self {
            tenant_id,
            agent_id: None,
            trace_id: Uuid::now_v7(),
            agent_token: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_tenant_generates_fresh_trace_id() {
        let t = Uuid::now_v7();
        let a = CallerContext::for_tenant(t);
        let b = CallerContext::for_tenant(t);
        assert_eq!(a.tenant_id, b.tenant_id);
        assert_ne!(a.trace_id, b.trace_id);
        assert!(a.agent_id.is_none());
        assert!(a.agent_token.is_none());
    }

    #[test]
    fn caller_context_round_trip() {
        let ctx = CallerContext {
            tenant_id: Uuid::now_v7(),
            agent_id: Some(Uuid::now_v7()),
            trace_id: Uuid::now_v7(),
            agent_token: Some("bearer-xyz".to_string()),
        };
        let json = serde_json::to_value(&ctx).unwrap();
        let back: CallerContext = serde_json::from_value(json).unwrap();
        assert_eq!(ctx, back);
    }

    #[test]
    fn serialise_caller_context_omits_none_fields() {
        let ctx = CallerContext::for_tenant(Uuid::now_v7());
        let s = serde_json::to_string(&ctx).unwrap();
        assert!(!s.contains("agent_id"));
        assert!(!s.contains("agent_token"));
        assert!(s.contains("trace_id"));
        assert!(s.contains("tenant_id"));
    }
}
