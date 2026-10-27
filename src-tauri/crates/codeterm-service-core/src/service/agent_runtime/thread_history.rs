use super::{engine, model::ItemCache};
use crate::multiplexer::types::TabBacking;
use crate::service::{ServiceContext, SpawnAgentParams};
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult, RuntimeThreadHistoryAction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

fn fork_history_ends_at(items: &[crate::agent::chat_types::ChatMsg], turn_id: &str) -> bool {
    items
        .iter()
        .rev()
        .find_map(|item| item.tool_flow_id.as_deref())
        == Some(turn_id)
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
    if request.action == RuntimeThreadHistoryAction::Fork
        && (context.active_turn_id.is_some() || !context.active_turns.is_empty())
    {
        return Err("An active source thread cannot be forked".into());
    }
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
    match request.action {
        RuntimeThreadHistoryAction::Fork => fork(svc, &session, current, request, source).await,
        RuntimeThreadHistoryAction::Revert => revert(svc, tab_id, &session, current, request).await,
    }
}

async fn fork(
    svc: &ServiceContext,
    source_session: &std::sync::Arc<engine::Session>,
    source_binding: ThreadHistoryBinding,
    request: ThreadHistoryRequest,
    source: (crate::multiplexer::types::Tab, String),
) -> Result<Value, String> {
    let (source_tab, workspace_id) = source;
    let cwd = source_tab.cwd.clone().ok_or("Source cwd unavailable")?;
    let params = SpawnAgentParams {
        agent: Some(source_session.provider.clone()),
        cwd: Some(cwd.clone()),
        workspace_id: Some(workspace_id.clone()),
        force_tab: Some(true),
        title: Some("Fork".into()),
        description: Some(format!("Fork of tab {}", source_tab.id)),
        model: source_tab
            .restorable_launch
            .model
            .clone()
            .or(source_tab.agent_provider_resolved_model.clone()),
        reasoning_effort: source_tab
            .restorable_launch
            .resolved_reasoning_effort
            .clone(),
        activate: Some(false),
        user_created: true,
        sandbox: source_tab
            .restorable_launch
            .execution_sandbox
            .as_ref()
            .map(|policy| {
                crate::service::execution_sandbox::requested_label(&policy.intent).into()
            }),
        spawned_by_tab_id: Some(source_tab.id.clone()),
        ..Default::default()
    };
    let owner = svc.clone();
    let source_thread_id = source_binding.thread_id.clone();
    let turn_id = request.turn_id.clone();
    let spawned = tokio::task::spawn_blocking(move || {
        owner.spawn_agent_sync_for_runtime_fork(params, source_thread_id, turn_id)
    })
    .await
    .map_err(|error| error.to_string())??;
    let fork_session = match engine::for_tab(&spawned.tab_id) {
        Some(session) => session,
        None => {
            close_fork_tab(svc, &spawned.tab_id).await;
            return Err("Fork child runtime did not bind its provider thread".into());
        }
    };
    if let Err(error) = engine::validate_binding(svc, source_session) {
        close_fork_tab(svc, &spawned.tab_id).await;
        return Err(error.to_string());
    }
    if let Err(error) = engine::validate_binding(svc, &fork_session) {
        close_fork_tab(svc, &spawned.tab_id).await;
        return Err(error.to_string());
    }
    let cutoff_verified =
        fork_history_ends_at(&fork_session.cache.lock().snapshot(), &request.turn_id);
    if !cutoff_verified {
        log::debug!(
            "[thread-history] fork={} rejected cutoff={}",
            spawned.tab_id,
            request.turn_id
        );
        close_fork_tab(svc, &spawned.tab_id).await;
        return Err("Fork child history does not end at the selected turn".into());
    }
    let forked_binding_result = {
        let context = fork_session.context.lock();
        binding(&context)
    };
    let forked_binding = match forked_binding_result {
        Ok(binding) => binding,
        Err(error) => {
            close_fork_tab(svc, &spawned.tab_id).await;
            return Err(error);
        }
    };
    let target = svc.mux.read().get_tab(&spawned.tab_id).cloned();
    let source_after = binding(&source_session.context.lock())?;
    let target_cwd = target.as_ref().and_then(|tab| tab.cwd.as_deref());
    if !fork_binding_valid(
        &source_binding,
        &source_after,
        &forked_binding.thread_id,
        &spawned.tab_id,
        &source_tab.id,
        &cwd,
        target_cwd,
        Some(forked_binding.binding_nonce.as_str()),
    ) {
        close_fork_tab(svc, &spawned.tab_id).await;
        return Err("Fork tab did not acquire a distinct runtime binding".into());
    }
    if let Err(error) = svc
        .name_fork_tab(&spawned.tab_id, &source_tab.title)
        .and_then(|()| svc.set_active_tab(&workspace_id, &spawned.tab_id))
    {
        close_fork_tab(svc, &spawned.tab_id).await;
        return Err(error);
    }
    Ok(json!({
        "tabId":spawned.tab_id,
        "threadId":forked_binding.thread_id,
        "turnId":request.turn_id
    }))
}

async fn revert(
    svc: &ServiceContext,
    tab_id: &str,
    session: &std::sync::Arc<engine::Session>,
    current: ThreadHistoryBinding,
    request: ThreadHistoryRequest,
) -> Result<Value, String> {
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime process unavailable")?;
    let mut changes = session.control_changes.subscribe();
    *session.reverted_thread.lock() = None;
    let result = engine::run(
        svc,
        &session.provider,
        session,
        &process,
        RuntimeOperation::Control {
            thread_id: Some(current.thread_id.clone()),
            action: "threadHistory".into(),
            value: json!({"action":"revert","turnId":request.turn_id}),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
    if session.context.lock().epoch != current.epoch {
        return Err("Thread history binding superseded".into());
    }
    if !matches!(result, RuntimeResult::Done) {
        return Err("Provider did not complete the revert".into());
    }
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
            let reverted = {
                let evidence = session.reverted_thread.lock();
                evidence.as_deref() == Some(current.thread_id.as_str())
            };
            if reverted {
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
    engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
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
    engine::publish(svc, tab_id, session);
    svc.emit_state_changed(crate::service::MuxEmitCause::RuntimeSignal);
    Ok(json!({
        "threadId":current.thread_id,
        "turnId":request.turn_id,
        "reverted":true
    }))
}

async fn close_fork_tab(svc: &ServiceContext, tab_id: &str) {
    let owner = svc.clone();
    let cleanup_tab = tab_id.to_string();
    let _ = tokio::task::spawn_blocking(move || owner.close_tab(&cleanup_tab)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_types::agent::chat_types::ChatMsg;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn fork_cut_requires_the_selected_turn_as_the_child_history_tail() {
        let row = |turn: &str| ChatMsg {
            tool_flow_id: Some(turn.into()),
            ..Default::default()
        };
        assert!(fork_history_ends_at(
            &[row("one"), row("two"), ChatMsg::default()],
            "two"
        ));
        assert!(!fork_history_ends_at(
            &[row("one"), row("two"), row("three")],
            "two"
        ));
        assert!(!fork_history_ends_at(&[row("one")], "two"));
        assert!(!fork_history_ends_at(&[], "two"));
    }

    fn binding() -> ThreadHistoryBinding {
        ThreadHistoryBinding {
            thread_id: "source-thread".into(),
            binding_nonce: "source-nonce".into(),
            epoch: 4,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fork_thread_is_created_and_bound_by_one_child_runtime_writer() {
        const PROVIDER: &str = "thread-fork-writer-fixture";
        let (mut mux, source_tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let cwd = crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned();
        {
            let source = mux.get_tab_mut(&source_tab_id).unwrap();
            source.agent_type = Some(PROVIDER.into());
            source.agent_session_id = Some("source-thread".into());
            source.session_binding_nonce = Some("source-nonce".into());
            source.reported_session_binding_nonce = Some("source-nonce".into());
            source.cwd = Some(cwd.clone());
            source.backing = TabBacking::ChatBackend {
                plugin_id: PROVIDER.into(),
                session_id: "source-thread".into(),
            };
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        let _storage = fixture.install_seeded_storage_desk();
        let mut cleanup = RuntimeCleanup {
            service: fixture.service.clone(),
            tabs: vec![source_tab_id.clone()],
        };
        fixture.service.chatbackend_registry.register(
            PROVIDER,
            Arc::new(
                crate::service::spawn_capability::test_backend::SpawnableTestBackend {
                    id: PROVIDER,
                },
            ),
        );
        let methods = json!({
            "initialize":["initialize"],
            "resumeThread":["thread/resume"],
            "readHistory":["thread/turns/list"],
            "forkThread":["thread/turns/list","thread/fork"]
        });
        let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(
            &json!({
                "id":PROVIDER,
                "version":"1.0.0",
                "displayName":"Fork fixture",
                "capabilities":{
                    "agentProvider":true,
                    "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                    "agentRuntime":{
                        "runtime":"stdio-jsonrpc",
                        "command":{"binary":"node","args":[format!("{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs",crate::test_support::fixture_manifest_dir().display())]},
                        "requestMethods":methods,
                        "notificationMethods":[],
                        "threadHistory":{"actions":["fork","revert"]}
                    }
                },
                "permissions":{"subprocess":{"allow":["node"]}}
            })
            .to_string(),
        )
        .unwrap();
        let source = r#"
module.exports = {
  runtimePlan(ctx, op) {
    if (op.kind === 'forkThread') {
      if (ctx.threadId || ctx.activeTurnId || ctx.activeTurns.length) throw Object.assign(new Error('Fork requires child runtime'),{code:'binding_mismatch'});
      return {action:{kind:'request',call:{method:'thread/turns/list',params:{threadId:op.sourceThreadId,itemsView:'notLoaded',sortDirection:'asc',limit:8}}},state:{stage:'turns'}};
    }
    const methods={initialize:'initialize',resumeThread:'thread/resume',readHistory:'thread/turns/list'};
    return {action:{kind:'request',call:{method:methods[op.kind],params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') return {kind:'complete',result:{kind:'initialized'}};
    if (op.kind === 'resumeThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:op.threadId,settings:op.settings}};
    if (op.kind === 'readHistory') return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:op.threadId === 'fork-thread' ? [{id:'fork-row',seq:0,type:'assistant',content:'answer',toolFlowId:'turn-two'}] : [],openItems:[]}};
    if (op.kind === 'forkThread' && state.stage === 'turns') {
      if (!reply.value.data.some(turn=>turn.id === op.turnId && turn.status !== 'inProgress')) return {kind:'refused',reason:{code:'provider_refused',message:'Selected turn unavailable'},beforeAcceptance:true};
      return {kind:'continue',plan:{action:{kind:'request',call:{method:'thread/fork',params:{threadId:op.sourceThreadId,lastTurnId:op.turnId}}},state:{stage:'fork'}},beforeAcceptance:false};
    }
    if (op.kind === 'forkThread' && state.stage === 'fork' && reply.value.thread.id === 'fork-thread') return {kind:'complete',result:{kind:'threadForked',sourceThreadId:op.sourceThreadId,threadId:'fork-thread',turnId:op.turnId}};
    return {kind:'refused',reason:{code:'provider_refused',message:'Unexpected fork fixture operation'}};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;
        fixture.service.agent_registry.register_provider(Box::new(
            codeterm_plugin::js_runtime::JsProvider::new(
                manifest,
                Arc::new(
                    codeterm_plugin::js_runtime::PluginVm::from_source(PROVIDER, source.into())
                        .unwrap(),
                ),
            ),
        ));
        let launch_service = fixture.service.clone();
        let launch_tab = source_tab_id.clone();
        tokio::task::spawn_blocking(move || {
            crate::service::agent_runtime::transport::launch(
                &launch_service,
                PROVIDER,
                &launch_tab,
                json!({}),
            )
        })
        .await
        .unwrap()
        .unwrap();
        let source_session = engine::for_tab(&source_tab_id).unwrap();
        source_session.cache.lock().reconcile(ChatMsg {
            id: "turn-two-row".into(),
            tool_flow_id: Some("turn-two".into()),
            ..Default::default()
        });
        let source_context = source_session.context.lock().clone();
        let source_history = source_session.cache.lock().snapshot();
        let result = operate(
            &fixture.service,
            &source_tab_id,
            ThreadHistoryRequest {
                action: RuntimeThreadHistoryAction::Fork,
                turn_id: "turn-two".into(),
                confirmed: false,
                binding: Some(ThreadHistoryBinding {
                    thread_id: source_context.thread_id.clone().unwrap(),
                    binding_nonce: source_context.binding_nonce.clone(),
                    epoch: source_context.epoch,
                }),
            },
        )
        .await
        .unwrap();
        let forked_tab_id = result["tabId"].as_str().unwrap().to_string();
        cleanup.tabs.push(forked_tab_id.clone());
        let child_session = engine::for_tab(&forked_tab_id).unwrap();
        let child_context = child_session.context.lock().clone();
        let source_process = source_session.process.lock().clone().unwrap();
        let child_process = child_session.process.lock().clone().unwrap();
        let mux = fixture.service.mux.read();
        let fork_thread_bindings = mux
            .snapshot()
            .tabs
            .values()
            .filter(|tab| tab.agent_session_id.as_deref() == Some("fork-thread"))
            .count();

        assert_eq!(child_context.thread_id.as_deref(), Some("fork-thread"));
        assert_eq!(child_context.tab_id, forked_tab_id);
        assert_ne!(child_context.binding_nonce, source_context.binding_nonce);
        assert_ne!(Arc::as_ptr(&source_process), Arc::as_ptr(&child_process));
        assert_eq!(fork_thread_bindings, 1);
        assert_eq!(
            source_session.context.lock().thread_id,
            source_context.thread_id
        );
        assert_eq!(
            source_session.context.lock().binding_nonce,
            source_context.binding_nonce
        );
        assert_eq!(source_session.cache.lock().snapshot(), source_history);
        drop(mux);
        drop(cleanup);
    }

    struct RuntimeCleanup {
        service: crate::service::ServiceContext,
        tabs: Vec<String>,
    }

    impl Drop for RuntimeCleanup {
        fn drop(&mut self) {
            for tab in &self.tabs {
                let _ = engine::stop_blocking(&self.service, tab);
            }
            crate::service::session_persist::stop_worker(&self.service)
                .expect("stop fork runtime persistence worker");
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
        assert!(truncate_after_evidence(
            &mut cache,
            "turn-two",
            "source-thread",
            Some("foreign-thread")
        )
        .is_err());
        assert_eq!(cache.snapshot(), before);
        assert!(truncate_after_evidence(
            &mut cache,
            "turn-two",
            "source-thread",
            Some("source-thread")
        )
        .is_ok());
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
