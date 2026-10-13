use super::{
    auth::AuthState,
    client::Client,
    lifecycle::{rpc_error, Spawn},
    model::RuntimeMode,
    transcript::History,
};
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
    tauri::async_runtime::spawn(async move {
        if let Ok(response) = sidecar
            .client
            .request("account/read", json!({"refreshToken":false}))
            .await
        {
            if super::lifecycle::for_tab(&tab)
                .is_some_and(|current| std::sync::Arc::ptr_eq(&current, &sidecar))
            {
                *sidecar.auth.lock() = super::auth::account_state(&response);
                svc.emit_state_changed();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_instant_start_fresh_reply_identity_needs_no_history_or_mcp_events() {
        let reply = json!({"thread":{"id":"own", "threadSource":"user"}});
        let (thread, history) = fresh_history(&reply).unwrap();
        assert_eq!(thread, "own");
        assert!(history.messages.is_empty());
        assert!(history.active_turn.is_none());
        assert!(history.completed_turns.is_empty());
        assert!(fresh_history(&json!({"thread":{"id":"own"}})).is_ok());
        assert!(fresh_history(&json!({"thread":{"id":"own", "threadSource":"appServer"}})).is_ok());
        assert!(fresh_history(
            &json!({"thread":{"id":"own","threadSource":"user","parentThreadId":"foreign"}})
        )
        .is_err());
    }

    #[test]
    fn codex_rpc_instant_start_accepts_optional_analytics_source_only_on_own_reply() {
        assert!(fresh_history(
            &json!({"thread":{"id":"own", "threadSource":null, "source":"appServer"}})
        )
        .is_ok());
        for reply in [
            json!({"thread":{"id":"own", "threadSource":"subagent"}}),
            json!({"thread":{"id":"own", "source":{"subAgent":{"thread_spawn":{}}}}}),
            json!({"thread":{"id":"", "threadSource":"user"}}),
            json!({"thread":{"threadSource":"user"}}),
        ] {
            assert!(fresh_history(&reply).is_err());
        }
    }

    #[tokio::test]
    async fn codex_rpc_instant_start_remote_resume_waits_for_materialization_reply() {
        use futures_util::FutureExt;
        let reply = json!({"thread":{"id":"own"}});
        let (ack, received) = tokio::sync::oneshot::channel();
        let prepared = materialized_fresh_history(&reply, |params| {
            assert_eq!(params["threadId"], "own");
            assert_eq!(params["items"].as_array().unwrap().len(), 1);
            assert_eq!(params["items"][0]["role"], "developer");
            assert_eq!(params["items"][0]["content"][0]["text"], BINDING_MARKER);
            async move { received.await.unwrap() }
        });
        tokio::pin!(prepared);
        assert!(prepared.as_mut().now_or_never().is_none());
        ack.send(Ok(json!({}))).unwrap();
        assert_eq!(prepared.await.unwrap().0, "own");
        let failed = materialized_fresh_history(&reply, |_| async {
            Err("materialization rejected".into())
        })
        .await;
        assert_eq!(failed.err().as_deref(), Some("materialization rejected"));
    }

    #[test]
    fn codex_rpc_instant_start_carries_resolved_model_effort_and_reviewer_explicitly() {
        let spawn = Spawn {
            provider: "opaque-provider".into(),
            cwd: "/cwd".into(),
            instructions: None,
            resume: None,
            model: Some("selected-model".into()),
            effort: Some("chosen-effort".into()),
            full_access: false,
        };
        let params = parameters(&spawn, RuntimeMode::Auto);
        assert_eq!(params["model"], "selected-model");
        assert_eq!(params["threadSource"], "user");
        assert_eq!(params["config"]["model_reasoning_effort"], "chosen-effort");
        assert_eq!(params["approvalsReviewer"], "auto_review");
        assert_eq!(params["approvalPolicy"], "on-request");
        assert_eq!(params["sandbox"], "workspace-write");
    }
}
