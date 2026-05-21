//! Intent dispatch pipeline.
//!
//! `dispatch_intent(envelope) -> ResponseEnvelope`:
//!   1. Build the `CallerContext` from the envelope (Phase 1: the
//!      `agent_token` is carried unverified; Cedar authz arrives Phase 5).
//!   2. Look up the loaded `CompiledConfig` from the `AppState`.
//!   3. Validate the intent against the entity schema (`eh-compiler`).
//!   4. Route to a `(binding, source_name)` (`eh-router`).
//!   5. Look up the connector for that source from the `ConnectorMap`.
//!   6. Dispatch `execute_read` or `execute_append` per the intent action.
//!   7. Translate every step's typed error into the structured
//!      `ErrorResponse` taxonomy.

use std::time::Instant;

use eh_connector_api::ConnectorError;
use eh_core::{Action, CallerContext, Intent};
use eh_protocol::{ErrorCode, ErrorResponse, IntentEnvelope, ResponseEnvelope};
use eh_router::RouterError;
use eh_telemetry::{label, metric_name};
use metrics::{counter, histogram};
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

use crate::state::AppState;

/// Execute one intent end-to-end.
#[instrument(
    skip(state, envelope),
    fields(
        entity = %envelope.intent.entity,
        action = ?envelope.intent.action,
    )
)]
pub async fn dispatch_intent(state: &AppState, envelope: IntentEnvelope) -> ResponseEnvelope {
    let started = Instant::now();
    let intent = envelope.intent;

    let ctx = CallerContext {
        tenant_id: state.default_tenant,
        agent_id: None, // populated in Phase 5 from Cedar principal lookup
        trace_id: Uuid::now_v7(),
        agent_token: Some(envelope.agent_token),
    };

    let response = run(state, intent.clone(), ctx).await;
    let outcome_label = if response.is_success() { "ok" } else { "error" };

    histogram!(
        metric_name::INTENT_LATENCY_MS,
        label::ENTITY => intent.entity.clone(),
        label::ACTION => action_label(intent.action),
        label::OUTCOME => outcome_label,
    )
    .record(started.elapsed().as_secs_f64() * 1000.0);

    counter!(
        metric_name::INTENT_COUNT,
        label::ENTITY => intent.entity.clone(),
        label::ACTION => action_label(intent.action),
        label::OUTCOME => outcome_label,
    )
    .increment(1);

    if let ResponseEnvelope::Error(ref err) = response {
        counter!(
            metric_name::INTENT_ERROR_COUNT,
            label::ENTITY => intent.entity.clone(),
            label::ACTION => action_label(intent.action),
            label::CODE => format!("{:?}", err.code).to_lowercase(),
        )
        .increment(1);
        warn!(
            target: "eh.dispatch",
            code = ?err.code,
            message = %err.message,
            "intent failed"
        );
    } else {
        info!(target: "eh.dispatch", "intent succeeded");
    }

    response
}

async fn run(state: &AppState, intent: Intent, ctx: CallerContext) -> ResponseEnvelope {
    let cfg = state.config.load();

    // Validate intent shape (cheap pre-flight).
    let entity = match cfg.entity(&intent.entity) {
        Some(e) => e.clone(),
        None => {
            return error(
                ErrorCode::UnknownEntity,
                format!("unknown entity {:?}", intent.entity),
            )
        }
    };
    if let Err(e) = eh_compiler::validate(&intent, &entity) {
        return error(ErrorCode::InvalidIntent, format!("{e}"));
    }

    // Route to a binding + source.
    let routed = match eh_router::route(&intent, &cfg) {
        Ok(r) => r,
        Err(e) => {
            return match e {
                RouterError::UnknownEntity(name) => {
                    error(ErrorCode::UnknownEntity, format!("unknown entity {name:?}"))
                }
                RouterError::NoRoute { .. } | RouterError::NoBindingForRoute { .. } => {
                    error(ErrorCode::NoBinding, format!("{e}"))
                }
                RouterError::ActionNotSupported { .. } => {
                    error(ErrorCode::Forbidden, format!("{e}"))
                }
            };
        }
    };

    // Look up the connector by source name.
    let connector = match state.connector_for(&routed.source_name) {
        Some(c) => c.clone(),
        None => {
            warn!(
                target: "eh.dispatch",
                source = %routed.source_name,
                "no connector instance built for source — operator misconfiguration"
            );
            return error(
                ErrorCode::ConfigError,
                format!(
                    "source {:?} has no connector instance (build feature missing or factory build failed)",
                    routed.source_name
                ),
            );
        }
    };

    debug!(
        target: "eh.dispatch",
        source = %routed.source_name,
        kind = %connector.kind(),
        action = ?intent.action,
        "dispatching"
    );

    let result = match intent.action {
        Action::Read => connector
            .execute_read(&routed.binding, &routed.entity, &intent, &ctx)
            .await
            .map(ResponseEnvelope::Success),
        Action::Append => connector
            .execute_append(&routed.binding, &routed.entity, &intent, &ctx)
            .await
            .map(|outcome| {
                // Surface the append outcome as a single-row artifact so
                // the wire shape stays uniform.
                let mut row = eh_core::ArtifactRow::new();
                row.insert("rows_inserted", serde_json::json!(outcome.rows_inserted));
                ResponseEnvelope::Success(eh_core::Artifact {
                    rows: vec![row],
                    source_kind: connector.kind().to_string(),
                    source_id: None,
                })
            }),
    };

    match result {
        Ok(env) => env,
        Err(e) => translate_connector_error(e),
    }
}

fn translate_connector_error(e: ConnectorError) -> ResponseEnvelope {
    match e {
        ConnectorError::Connect(msg) => {
            ResponseEnvelope::Error(ErrorResponse::new(ErrorCode::ConnectorError, msg))
        }
        ConnectorError::Unhealthy(msg) => {
            ResponseEnvelope::Error(ErrorResponse::new(ErrorCode::ConnectorError, msg))
        }
        ConnectorError::InvalidIntent(msg) => {
            ResponseEnvelope::Error(ErrorResponse::new(ErrorCode::InvalidIntent, msg))
        }
        ConnectorError::EngineRefusal(msg) => {
            // Per zero-trust §12: this is the debugging surface working
            // as designed. Surface it with a clear label.
            ResponseEnvelope::Error(ErrorResponse::new(
                ErrorCode::Forbidden,
                format!("engine refused operation: {msg}"),
            ))
        }
        ConnectorError::TypeMismatch { .. } => {
            ResponseEnvelope::Error(ErrorResponse::new(ErrorCode::InvalidIntent, format!("{e}")))
        }
        ConnectorError::Backend(msg) => {
            ResponseEnvelope::Error(ErrorResponse::new(ErrorCode::ConnectorError, msg))
        }
        ConnectorError::Unsupported(action) => ResponseEnvelope::Error(ErrorResponse::new(
            ErrorCode::Forbidden,
            format!("connector does not support {action:?}"),
        )),
    }
}

fn error(code: ErrorCode, message: impl Into<String>) -> ResponseEnvelope {
    ResponseEnvelope::Error(ErrorResponse::new(code, message))
}

fn action_label(action: Action) -> &'static str {
    match action {
        Action::Read => "read",
        Action::Append => "append",
    }
}
