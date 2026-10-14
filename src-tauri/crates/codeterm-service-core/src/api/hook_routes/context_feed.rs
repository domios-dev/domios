use crate::api::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_types::agent::context::TurnContext;
use serde_json::{json, Value};
use tauri::Manager;

pub(crate) fn merge(
    previous: Option<&TurnContext>,
    mut incoming: TurnContext,
    configured_window: Option<u64>,
) -> Option<TurnContext> {
    if !incoming.valid() {
        return None;
    }
    if incoming.window_tokens == 0 {
        incoming.window_tokens = configured_window
            .filter(|window| *window > 0 && *window <= 10_000_000)
            .or_else(|| previous.map(|context| context.window_tokens))
            .unwrap_or(0);
    }
    if incoming.cost_usd.is_none() {
        incoming.cost_usd = previous.and_then(|context| context.cost_usd);
    }
    Some(incoming)
}

pub(crate) fn apply(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    incoming: TurnContext,
) -> bool {
    update(svc, tab_id, session_id, nonce, Some(incoming))
}

fn update(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    incoming: Option<TurnContext>,
) -> bool {
    if session_id.is_empty() || nonce.is_empty() {
        return false;
    }
    let Some(config) = svc.app_handle.try_state::<crate::config::ConfigState>() else {
        return false;
    };
    let provider_id = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let (enabled, window) = {
        let config = config.lock();
        let provider = provider_id
            .as_ref()
            .and_then(|provider| config.agents.get(provider));
        (
            provider
                .and_then(|provider| provider.extra.get("context_feed"))
                .and_then(toml::Value::as_bool)
                == Some(true),
            provider
                .and_then(|provider| provider.extra.get("context_window_tokens"))
                .and_then(toml::Value::as_integer)
                .and_then(|value| u64::try_from(value).ok()),
        )
    };
    if !enabled {
        return false;
    }
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return false;
    };
    if tab.agent_session_id.as_deref() != Some(session_id)
        || tab.session_binding_nonce.as_deref() != Some(nonce)
        || tab.agent_type != provider_id
    {
        return false;
    }
    let context = match incoming {
        Some(incoming) => match merge(tab.context.as_ref(), incoming, window) {
            Some(context) => Some(context),
            None => return false,
        },
        None => None,
    };
    if tab.context == context {
        return false;
    }
    tab.context = context;
    drop(mux);
    svc.emit_state_changed();
    true
}

pub(crate) fn apply_snapshot(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    nonce: Option<&str>,
    context: Option<&TurnContext>,
) {
    if let (Some(nonce), Some(context)) = (nonce, context) {
        apply(svc, tab_id, session_id, nonce, context.clone());
    }
}

pub(crate) async fn ingest(
    State(state): State<AppState>,
    Path(tab_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let provider = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let context = provider.as_deref().and_then(|provider| {
        state
            .svc
            .agent_registry
            .with_provider(provider, |provider| provider.parse_context_usage(&body))
            .flatten()
    });
    let changed = if body
        .pointer("/context_window/current_usage")
        .is_some_and(Value::is_null)
    {
        update(
            &state.svc,
            &tab_id,
            body.get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            super::header_str(&headers, "x-codeterm-binding-nonce").unwrap_or_default(),
            None,
        )
    } else {
        context.is_some_and(|context| {
            apply(
                &state.svc,
                &tab_id,
                body.get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                super::header_str(&headers, "x-codeterm-binding-nonce").unwrap_or_default(),
                context,
            )
        })
    };
    Ok(Json(json!({"ok":true,"changed":changed})))
}

#[cfg(test)]
mod tests;
