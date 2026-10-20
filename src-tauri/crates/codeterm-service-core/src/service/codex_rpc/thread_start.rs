use super::{
    auth::AuthState,
    client::Client,
    lifecycle::{rpc_error, Spawn},
    transcript::History,
};
use crate::service::agent_runtime::model::RuntimeMode;
use crate::service::MuxEmitCause;
use serde_json::{json, Value};

pub(super) fn parameters(spawn: &Spawn, mode: RuntimeMode) -> Value {
    let overrides = mode.overrides();
    let mut params = json!({
        "cwd": spawn.cwd,
        "developerInstructions": super::compaction::developer_instructions(spawn.instructions.as_deref()),
        "model": spawn.model,
        "threadSource": "user",
        "config": {},
        "approvalPolicy": overrides["approvalPolicy"],
        "approvalsReviewer": overrides["approvalsReviewer"],
        "sandbox": match mode {
            RuntimeMode::ApprovalRequired => "read-only",
            RuntimeMode::FullAccess => "danger-full-access",
            _ => "workspace-write",
        }
    });
    if let Some(effort) = &spawn.effort {
        params["config"]["model_reasoning_effort"] = json!(effort);
    }
    if spawn.host_owned.questions {
        params["config"]["features.default_mode_request_user_input"] = json!(false);
    }
    params
}

fn fresh_history(response: &Value) -> Result<(String, History), String> {
    let thread = response["thread"]["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("Missing new thread identity")?;
    let metadata = &response["thread"];
    let child = metadata
        .get("parentThreadId")
        .is_some_and(|parent| !parent.is_null())
        || metadata
            .get("source")
            .and_then(Value::as_object)
            .is_some_and(|source| source.contains_key("subAgent"));
    let analytics_child = metadata
        .get("threadSource")
        .and_then(Value::as_str)
        .is_some_and(|source| source.to_ascii_lowercase().starts_with("subagent"));
    if child || analytics_child {
        return Err("Sidecar thread identity unverified".into());
    }
    Ok((
        thread.to_owned(),
        History {
            messages: Vec::new(),
            active_turn: None,
            open_items: Default::default(),
            completed_turns: Vec::new(),
            final_assistant_ids: Default::default(),
        },
    ))
}

const BINDING_MARKER: &str = "Domios thread binding marker.";

async fn materialized_fresh_history<F>(
    response: &Value,
    inject: impl FnOnce(Value) -> F,
) -> Result<(String, History), String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    let (thread, history) = fresh_history(response)?;
    inject(json!({"threadId":thread,"items":[{
        "type":"message","role":"developer",
        "content":[{"type":"input_text","text":BINDING_MARKER}]
    }]}))
    .await?;
    Ok((thread, history))
}

pub(super) async fn initial_state(
    client: &Client,
    response: Value,
    resumed: bool,
) -> Result<(String, AuthState, History), String> {
    if !resumed {
        let (thread, history) = materialized_fresh_history(&response, |params| async move {
            client
                .request("thread/inject_items", params)
                .await
                .map_err(rpc_error)
        })
        .await?;
        return Ok((thread, AuthState::Unknown, history));
    }
    let response: super::schema::ThreadResponse =
        serde_json::from_value(response).map_err(|error| error.to_string())?;
    let thread = response.thread.id;
    let read = client
        .request(
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        )
        .await
        .map_err(rpc_error)?;
    if !super::routing::verified_user_thread(&thread, &read) {
        client.close("Thread identity unverified");
        return Err("Sidecar thread identity unverified".into());
    }
    let auth = client
        .request("account/read", json!({"refreshToken":false}))
        .await
        .map(|response| super::auth::account_state(&response))
        .unwrap_or_default();
    let history = super::transcript::history(client, &thread).await?;
    Ok((thread, auth, history))
}

pub(super) fn refresh_account(
    svc: crate::service::ServiceContext,
    tab: String,
    sidecar: std::sync::Arc<super::lifecycle::Sidecar>,
) {
    crate::async_runtime::spawn(async move {
        if let Ok(response) = sidecar
            .client
            .request("account/read", json!({"refreshToken":false}))
            .await
        {
            if super::lifecycle::for_tab(&tab)
                .is_some_and(|current| std::sync::Arc::ptr_eq(&current, &sidecar))
            {
                *sidecar.auth.lock() = super::auth::account_state(&response);
                svc.emit_state_changed(MuxEmitCause::ProviderSignal);
            }
        }
    });
}

#[cfg(test)]
mod tests;
