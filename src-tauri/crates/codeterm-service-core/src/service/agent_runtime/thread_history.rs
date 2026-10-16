use super::{
    engine,
    model::{ItemCache, RuntimeFailure},
};
use crate::multiplexer::types::TabBacking;
use crate::service::{ServiceContext, SpawnAgentParams};
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult, RuntimeThreadHistoryAction};
use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadHistoryBinding {
    pub thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadHistoryRequest {
    pub action: RuntimeThreadHistoryAction,
    pub turn_id: String,
    #[serde(default)]
    pub confirmed: bool,
    pub binding: Option<ThreadHistoryBinding>,
}

fn binding(
    context: &codeterm_plugin::agent_runtime::RuntimeContext,
) -> Result<ThreadHistoryBinding, String> {
    Ok(ThreadHistoryBinding {
        thread_id: context
            .thread_id
            .clone()
            .ok_or("Runtime thread unavailable")?,
        binding_nonce: context.binding_nonce.clone(),
        epoch: context.epoch,
    })
}

fn authorize(
    request: &ThreadHistoryRequest,
    capability: &codeterm_plugin::agent_runtime::RuntimeThreadHistoryCapability,
    current: &ThreadHistoryBinding,
) -> Result<(), String> {
    if !capability.actions.contains(&request.action) {
        return Err("Thread history action unsupported by provider".into());
    }
    if request.turn_id.trim().is_empty() || request.turn_id.len() > 4096 {
        return Err("Invalid turn boundary".into());
    }
    if request.binding.as_ref() != Some(current) {
        return Err("Thread history binding changed; refresh and try again".into());
    }
    if request.action == RuntimeThreadHistoryAction::Revert && !request.confirmed {
        return Err("Revert requires owner confirmation".into());
    }
    Ok(())
}

fn fork_binding_valid(
    source: &ThreadHistoryBinding,
    source_after: &ThreadHistoryBinding,
    forked_thread: &str,
    forked_tab: &str,
    source_tab: &str,
    source_cwd: &str,
    forked_cwd: Option<&str>,
    forked_nonce: Option<&str>,
) -> bool {
    source == source_after
        && !forked_tab.is_empty()
        && forked_tab != source_tab
        && forked_cwd == Some(source_cwd)
        && forked_thread != source.thread_id
        && forked_nonce.is_some_and(|nonce| !nonce.is_empty() && nonce != source.binding_nonce)
}

fn truncate_after_evidence(
    cache: &mut ItemCache,
    turn_id: &str,
    thread_id: &str,
    evidence_thread_id: Option<&str>,
) -> Result<(), String> {
    if evidence_thread_id != Some(thread_id) {
        return Err("Provider did not confirm thread/reverted".into());
    }
    if !cache.truncate_after_turn(turn_id) {
        return Err("Revert evidence arrived but the selected transcript boundary changed".into());
    }
    Ok(())
}

pub async fn operate(
    svc: &ServiceContext,
    tab_id: &str,
    request: ThreadHistoryRequest,
) -> Result<Value, String> {
    let session = engine::ensure(svc, tab_id)
        .await
        .map_err(|error| error.to_string())?;
    let _permit = session
        .gate
        .try_acquire()
        .map_err(|_| "Runtime operation already in progress")?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    let context = session.context.lock().clone();
    let current = binding(&context)?;
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|provider| provider.agent_runtime())
        .and_then(|capability| capability.thread_history);
    let Some(capability) = capability else {
        return Err("Thread history actions unsupported by provider".into());
    };
    authorize(&request, &capability, &current)?;
    if !session.cache.lock().has_turn(&request.turn_id) {
        return Err("Selected turn is no longer in the visible transcript".into());
    }
    let source = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).cloned().ok_or("Source tab not found")?;
        if !matches!(&tab.backing, TabBacking::ChatBackend { .. }) {
            return Err("Thread history requires a chat runtime tab".into());
        }
        let workspace = mux
            .snapshot()
            .workspaces
            .into_iter()
            .find(|workspace| workspace.tabs.contains(&tab.id))
            .map(|workspace| workspace.id)
            .ok_or("Source workspace unavailable")?;
        (tab, workspace)
    };
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime process unavailable")?;
    let action = match request.action {
        RuntimeThreadHistoryAction::Fork => "fork",
        RuntimeThreadHistoryAction::Revert => "revert",
    };
    let mut changes = session.control_changes.subscribe();
    if request.action == RuntimeThreadHistoryAction::Revert {
        *session.reverted_thread.lock() = None;
    }
    let result = engine::run(
        svc,
        &session.provider,
        &session,
        &process,
        RuntimeOperation::Control {
            thread_id: Some(current.thread_id.clone()),
            action: "threadHistory".into(),
            value: json!({"action":action,"turnId":request.turn_id}),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    if session.context.lock().epoch != current.epoch {
        return Err("Thread history binding superseded".into());
    }
    match request.action {
        RuntimeThreadHistoryAction::Fork => {
            let RuntimeResult::ThreadForked {
                source_thread_id,
                thread_id,
                turn_id,
            } = result
            else {
                return Err("Provider did not confirm the forked thread".into());
            };
            if source_thread_id != current.thread_id || turn_id != request.turn_id {
                return Err("Fork result differs from the selected source boundary".into());
            }
            let (source_tab, workspace_id) = source;
            let cwd = source_tab.cwd.clone().ok_or("Source cwd unavailable")?;
            let params = SpawnAgentParams {
                agent: Some(session.provider.clone()),
                cwd: Some(cwd),
                workspace_id: Some(workspace_id),
                force_tab: Some(true),
                title: Some("Fork".into()),
                description: Some(format!("Fork of tab {}", source_tab.id)),
                resume_session_id: Some(thread_id.clone()),
                model: source_tab
                    .restorable_launch
                    .model
                    .clone()
                    .or(source_tab.agent_provider_resolved_model.clone()),
                reasoning_effort: source_tab
                    .restorable_launch
                    .resolved_reasoning_effort
                    .clone(),
                activate: Some(true),
                user_created: true,
                spawned_by_tab_id: Some(source_tab.id.clone()),
                ..Default::default()
            };
            let owner = svc.clone();
            let spawned = tokio::task::spawn_blocking(move || owner.spawn_agent_sync(params))
                .await
                .map_err(|error| error.to_string())??;
            let fork_session = match engine::ensure(svc, &spawned.tab_id).await {
                Ok(session) => session,
                Err(error) => {
                    close_fork_tab(svc, &spawned.tab_id).await;
                    return Err(error.to_string());
                }
            };
            if let Err(error) = engine::validate_binding(svc, &session) {
                close_fork_tab(svc, &spawned.tab_id).await;
                return Err(error.to_string());
            }
            if let Err(error) = engine::validate_binding(svc, &fork_session) {
                close_fork_tab(svc, &spawned.tab_id).await;
                return Err(error.to_string());
            }
            let forked_binding = match binding(&fork_session.context.lock()) {
                Ok(binding) => binding,
                Err(error) => {
                    close_fork_tab(svc, &spawned.tab_id).await;
                    return Err(error);
                }
            };
            let target = svc.mux.read().get_tab(&spawned.tab_id).cloned();
            let source_after = binding(&session.context.lock())?;
            let target_cwd = target.as_ref().and_then(|tab| tab.cwd.as_deref());
            if forked_binding.thread_id != thread_id
                || !fork_binding_valid(
                    &current,
                    &source_after,
                    &forked_binding.thread_id,
                    &spawned.tab_id,
                    &source_tab.id,
                    &cwd,
                    target_cwd,
                    Some(&forked_binding.binding_nonce),
                )
            {
                close_fork_tab(svc, &spawned.tab_id).await;
                return Err("Fork tab did not acquire a distinct runtime binding".into());
            }
            Ok(json!({"tabId":spawned.tab_id,"threadId":thread_id,"turnId":turn_id}))
        }
        RuntimeThreadHistoryAction::Revert => {
            if !matches!(result, RuntimeResult::Done) {
                return Err("Provider did not complete the revert".into());
            }
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
                    if session.reverted_thread.lock().as_deref() == Some(current.thread_id.as_str())
                    {
                        return Ok::<(), String>(());
                    }
                    changes
                        .changed()
                        .await
                        .map_err(|_| "Runtime event stream closed".to_string())?;
                }
            })
            .await
            .map_err(|_| "Provider did not confirm thread/reverted".to_string())??;
            engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
            if session.context.lock().epoch != current.epoch {
                return Err("Thread history binding superseded".into());
            }
            let evidence_thread_id = session.reverted_thread.lock().clone();
            truncate_after_evidence(
                &mut session.cache.lock(),
                &request.turn_id,
                &current.thread_id,
                evidence_thread_id.as_deref(),
            )?;
            engine::publish(svc, tab_id, &session);
            svc.emit_state_changed(crate::service::MuxEmitCause::RuntimeSignal);
            Ok(json!({"threadId":current.thread_id,"turnId":request.turn_id,"reverted":true}))
        }
    }
}

async fn close_fork_tab(svc: &ServiceContext, tab_id: &str) {
    let owner = svc.clone();
    let cleanup_tab = tab_id.to_string();
    let _ = tokio::task::spawn_blocking(move || owner.close_tab(&cleanup_tab)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ThreadHistoryBinding {
        ThreadHistoryBinding {
            thread_id: "source-thread".into(),
            binding_nonce: "source-nonce".into(),
            epoch: 4,
        }
    }

    #[test]
    fn fork_binding_is_new_and_source_binding_is_unchanged() {
        let source = binding();
        assert!(fork_binding_valid(
            &source,
            &source,
            "fork-thread",
            "fork-tab",
            "source-tab",
            "/work/project",
            Some("/work/project"),
            Some("fork-nonce")
        ));
        assert!(!fork_binding_valid(
            &source,
            &source,
            "source-thread",
            "fork-tab",
            "source-tab",
            "/work/project",
            Some("/work/project"),
            Some("fork-nonce")
        ));
        let mut stale = source.clone();
        stale.epoch += 1;
        assert!(!fork_binding_valid(
            &source,
            &stale,
            "fork-thread",
            "fork-tab",
            "source-tab",
            "/work/project",
            Some("/work/project"),
            Some("fork-nonce")
        ));
        assert!(!fork_binding_valid(
            &source,
            &source,
            "fork-thread",
            "fork-tab",
            "source-tab",
            "/work/project",
            Some("/work/other"),
            Some("fork-nonce")
        ));
    }

    #[test]
    fn revert_requires_owner_confirmation_and_exact_current_binding() {
        let capability = codeterm_plugin::agent_runtime::RuntimeThreadHistoryCapability {
            actions: vec![RuntimeThreadHistoryAction::Revert],
        };
        let mut request = ThreadHistoryRequest {
            action: RuntimeThreadHistoryAction::Revert,
            turn_id: "turn-two".into(),
            confirmed: false,
            binding: Some(binding()),
        };
        assert!(authorize(&request, &capability, &binding()).is_err());
        request.confirmed = true;
        assert!(authorize(&request, &capability, &binding()).is_ok());
        let mut foreign = binding();
        foreign.thread_id = "foreign-thread".into();
        request.binding = Some(foreign);
        assert!(authorize(&request, &capability, &binding()).is_err());
        let mut stale = binding();
        stale.epoch += 1;
        request.binding = Some(stale);
        assert!(authorize(&request, &capability, &binding()).is_err());
    }

    #[test]
    fn revert_history_changes_only_after_matching_provider_evidence() {
        let mut cache = ItemCache::default();
        for (id, turn_id) in [
            ("turn-one", "turn-one"),
            ("turn-two", "turn-two"),
            ("turn-three", "turn-three"),
        ] {
            cache.reconcile(ChatMsg {
                id: id.into(),
                tool_flow_id: Some(turn_id.into()),
                ..Default::default()
            });
        }
        let before = cache.snapshot();
        assert!(truncate_after_evidence(&mut cache, "turn-two", "source-thread", None).is_err());
        assert_eq!(cache.snapshot(), before);
        assert!(
            truncate_after_evidence(
                &mut cache,
                "turn-two",
                "source-thread",
                Some("foreign-thread")
            )
            .is_err()
        );
        assert_eq!(cache.snapshot(), before);
        assert!(
            truncate_after_evidence(
                &mut cache,
                "turn-two",
                "source-thread",
                Some("source-thread")
            )
            .is_ok()
        );
        assert_eq!(
            cache
                .snapshot()
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-one", "turn-two"]
        );
    }
}
