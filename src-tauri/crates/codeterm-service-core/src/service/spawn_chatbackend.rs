//! Spawn path for non-PTY `chatBackend` tabs (`pty_id: None`, no backend
//! reader); inbound transcript arrives via the plugin `poll` → `StructuredSink`.

use crate::service::MuxEmitCause;
use std::collections::{HashMap, HashSet};
use std::future::Future;

#[path = "spawn_chatbackend/launch_options.rs"]
mod launch_options;
use launch_options::runtime_launch_mode;
pub(super) mod live_transcript;
mod role;
#[path = "spawn_chatbackend/runtime_launch.rs"]
mod runtime_launch;
use runtime_launch::ChatBackendRuntimeLaunch;
#[cfg(test)]
use runtime_launch::{runtime_initial_payload, runtime_pending_payload, RuntimeFirstTurn};
#[path = "spawn_chatbackend/watcher.rs"]
mod watcher;
use watcher::*;

use super::agent_runtime::transport::LaunchContext;
use super::spawn::{SpawnTabSpec, TabCommand, TabHandle};
use super::ServiceContext;
use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::agent::reports::ReportDetails;
use crate::multiplexer::types::{TabBacking, WatcherPolicy, WatcherSpec};
use crate::service::messenger::MessageEnvelope;
use crate::service::watcher::{
    apply_guardrails, build_observation, collect_watch_group, observation_config_from_json,
    parse_verdict, ObservationSources, VerdictOutcome, WatcherAction, WatcherVerdictStatus,
};

/// Spawn on the daemon's global tauri runtime. The synchronous socket-RPC spawn
/// path has no ambient Tokio runtime, where bare `tokio::spawn` would panic.
fn spawn_bg<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    crate::async_runtime::spawn(fut);
}

fn stamp_chatbackend_launch_identity(
    tab: &mut crate::multiplexer::types::Tab,
    metadata: &super::spawn::ChatBackendLaunchMetadata,
) {
    super::spawn::stamp_launch_authority(tab, &metadata.params);
    let agent = &metadata.agent;
    tab.restorable_launch.account_id = agent.account_id.clone();
    tab.restorable_launch.account_namespace = agent.account_namespace.clone();
    tab.worker_id = agent.worker_id.clone();
    tab.provisioned_cwd = agent.provisioned_cwd.clone();
    tab.report_target_tab_id = agent.report_target_tab_id.clone();
    tab.spawned_by_tab_id = agent.spawned_by_tab_id.clone();
    tab.agent_type = Some(agent.provider.clone());
    tab.agent_planned_model = agent.planned_model.clone();
    tab.agent_requested_model = agent.model.clone();
    tab.agent_provider_resolved_model = agent.model.clone();
    super::reasoning_effort::persist_launch_identity(
        tab,
        agent.planned_reasoning_effort.as_deref(),
        agent.reasoning_effort.as_deref(),
    );
}

fn runtime_resume_thread(
    existing: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    match (existing, requested) {
        (Some(existing), Some(requested)) if existing != requested => {
            Err("Runtime launch cannot replace the exact bound thread".into())
        }
        (Some(thread), _) | (None, Some(thread)) => Ok(Some(thread.into())),
        (None, None) => Ok(None),
    }
}

impl ServiceContext {
    /// Returns once the tab exists; the provider runtime starts in the background.
    pub(crate) fn spawn_chatbackend_with_ready(
        &self,
        spec: SpawnTabSpec,
        emit_state_change: bool,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<TabHandle, String> {
        let require_signed_in = ready_tx.is_some()
            && spec
                .chatbackend_launch
                .as_ref()
                .is_some_and(|launch| launch.agent.remote_task_bound);
        let (handle, launch) = match self.stage_chatbackend_spawn(spec, emit_state_change) {
            Ok(staged) => staged,
            Err(error) => {
                if let Some(ready_tx) = ready_tx {
                    let _ = ready_tx.send(Err(error.clone()));
                }
                return Err(error);
            }
        };
        if !launch.can_defer() {
            let readiness = launch.run_with_auth(require_signed_in);
            if let Some(ready_tx) = ready_tx {
                let _ = ready_tx.send(readiness.clone());
            }
            return readiness.map(|()| handle);
        }
        let svc = self.clone();
        let tab_id = handle.tab_id.clone();
        let cron_owned = launch.cron_owned;
        std::thread::spawn(move || {
            let recipient = svc.spawn_failure_recipient(&tab_id);
            let readiness = launch.run_with_auth(require_signed_in);
            match ready_tx {
                // A dispatch owns its rollback and failure event once readiness arrives.
                Some(ready_tx) => {
                    let _ = ready_tx.send(readiness);
                }
                None if !cron_owned => {
                    if let Err(error) = readiness {
                        svc.report_spawn_runtime_failure(recipient, &tab_id, &error);
                    }
                }
                None => {}
            }
        });
        Ok(handle)
    }

    /// Read before the launch, which may roll the tab back on failure.
    pub(crate) fn spawn_failure_recipient(&self, tab_id: &str) -> Option<(String, String)> {
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id)?;
        Some((tab.parent_tab_id.clone()?, tab.title.clone()))
    }

    pub(crate) fn report_spawn_runtime_failure(
        &self,
        recipient: Option<(String, String)>,
        tab_id: &str,
        error: &str,
    ) {
        log::warn!("[spawn] tab={tab_id} provider runtime failed to start: {error}");
        let Some((parent_tab_id, title)) = recipient else {
            return;
        };
        let message = format!("{title} could not start its provider runtime: {error}");
        let report = self.report_store.lock().add_detailed(
            &parent_tab_id,
            tab_id,
            &title,
            &message,
            ReportDetails::default(),
            None,
        );
        let gate = self.hierarchy_report_recipient_gate(&parent_tab_id);
        self.events.broadcast(
            "agent.report",
            serde_json::json!({ "target_tab_id": parent_tab_id, "report": report.clone() }),
        );
        self.deliver_hierarchy_report_if_allowed(&gate, &report, self);
    }

    #[cfg(test)]
    pub(crate) fn spawn_chatbackend_impl_with_state_change(
        &self,
        spec: SpawnTabSpec,
        emit_state_change: bool,
    ) -> Result<TabHandle, String> {
        let (handle, launch) = self.stage_chatbackend_spawn(spec, emit_state_change)?;
        launch.run()?;
        Ok(handle)
    }

    pub(crate) fn stage_chatbackend_spawn(
        &self,
        spec: SpawnTabSpec,
        emit_state_change: bool,
    ) -> Result<(TabHandle, ChatBackendRuntimeLaunch), String> {
        let taskless_authorized_gates = spec
            .chatbackend_launch
            .as_ref()
            .map(|launch| launch.params.authorized_gates.clone())
            .unwrap_or_default();
        let (plugin_id, mut config, role, role_profile, can_spawn, watcher) = match &spec.command {
            TabCommand::ChatBackend {
                plugin_id,
                config,
                role,
                role_profile,
                can_spawn,
                watcher,
            } => (
                plugin_id.clone(),
                config.clone(),
                role.clone(),
                role_profile.clone(),
                *can_spawn,
                watcher.clone(),
            ),
            _ => return Err("spawn_chatbackend_impl called without ChatBackend command".into()),
        };

        let cloud = self.remote_execution(&plugin_id, &config)?;
        let local_runtime = !cloud && super::agent_runtime::engine::requested(self, &plugin_id);
        let router_owner =
            live_transcript::router_launch_owner(self, &spec, &plugin_id, local_runtime)?;
        let config_model = config
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if local_runtime {
            if let Some(metadata) = spec.chatbackend_launch.as_ref() {
                super::spawn_capability::validate_runtime_launch_arguments(
                    &plugin_id,
                    &metadata.agent.extra_args,
                )?;
            }
            let params = launch_options::authorize(
                self,
                &plugin_id,
                spec.chatbackend_launch
                    .as_ref()
                    .map(|metadata| &metadata.params),
            )?;
            let tier = super::spawn_speed::validate_agent_speed(
                &self.agent_registry,
                &plugin_id,
                params.speed.as_deref(),
            )?;
            let full_access = params.permission_bypass_requested(&plugin_id);
            if config.is_null() {
                config = serde_json::json!({});
            }
            let resume = spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.agent.resume_session_id.as_deref());
            launch_options::apply_model(self, &plugin_id, &mut config, resume)?;
            config
                .as_object_mut()
                .ok_or("Runtime spawn config must be an object")?
                .insert(
                    "mode".into(),
                    serde_json::json!(runtime_launch_mode(full_access)),
                );
            if let Some(tier) = tier {
                config["serviceTier"] = serde_json::json!(tier);
            }
        } else if spec
            .chatbackend_launch
            .as_ref()
            .is_some_and(|launch| launch.agent.worker_id.is_some())
        {
            return Err(
                super::spawn_capability::SpawnRejection::CapabilityUnavailable {
                    requested: plugin_id,
                    missing: super::spawn_capability::MissingSpawnCapability::WorkerIdentityHandoff,
                }
                .to_string(),
            );
        }

        let (ws_id, ws_cwd) = {
            let mux = self.mux.read();
            let id = crate::service::spawn::resolve_spawn_workspace_id(
                spec.workspace_id.clone(),
                spec.parent_tab_id.as_deref(),
                spec.creator_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let (resolved_cwd, warning) = if local_runtime {
            Self::resolve_agent_cwd(&spec.cwd, &ws_cwd)
        } else {
            (
                spec.cwd
                    .clone()
                    .or(ws_cwd)
                    .map(|cwd| Self::normalize_resolved_agent_cwd(&cwd)),
                None,
            )
        };
        let team_attachment = crate::service::team_membership::resolve_spawn_attachment(
            &self.mux.read(),
            spec.team_id.as_deref(),
            spec.creator_tab_id.as_deref(),
            &ws_id,
        )?;

        let allocation =
            if let Some(super::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) = spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.owner_linkage.as_ref())
            {
                let cron = self
                    .try_ext::<crate::CronState>()
                    .ok_or("Cron state unavailable before provisional allocation")?;
                self.run_watcher
                    .reserve_cron_slot(self, cron, job_id, run_id)?;
                Some(super::spawn::cron_launch::CronAllocation::claim(
                    self, job_id, run_id,
                )?)
            } else {
                None
            };
        let gate = self.limit_gate();
        let (tab_id, original) = {
            let mut mux = self.mux.write();
            if let Some(owner) = router_owner {
                mux.ensure_tab_membership_mutable(owner)?;
                if mux.find_workspace_for_tab(owner).as_deref() != Some(ws_id.as_str())
                    || mux
                        .get_tab(owner)
                        .is_none_or(|tab| tab.is_transcript_router())
                    || mux
                        .all_tabs()
                        .any(|tab| tab.transcript_router_owner() == Some(owner))
                {
                    return Err("Transcript router owner changed or already has a router".into());
                }
            }
            let result = match spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.params.existing_tab_id.as_deref())
            {
                Some(existing) => mux.claim_existing_tab(&ws_id, existing)?,
                None => {
                    super::billing::admit_new_tab(&gate, &mux)?;
                    let tab_id = mux.create_tab(&ws_id)?;
                    crate::startup::boot_timing::bind_request(&tab_id);
                    tab_id
                }
            };
            if let Some(owner) = router_owner {
                let tab = mux
                    .get_tab_mut(&result)
                    .ok_or("Transcript router allocation lost")?;
                tab.audience = crate::multiplexer::types::TabAudience::TranscriptRouter;
                tab.spawned_by_tab_id = Some(owner.into());
                tab.hidden = true;
                tab.show_in_tabbar = false;
                tab.skip_snapshot = true;
                if let Some(workspace) = mux.get_workspace_mut(&ws_id) {
                    if workspace.active_tab_id.as_deref() == Some(result.as_str()) {
                        workspace.active_tab_id = None;
                    }
                }
            }
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_tab(&result, title) {
                    log::warn!("[spawn.chatbackend] rename_tab failed: {}", e);
                }
            }
            if let Some(ref description) = spec.description {
                if spec.task_description_managed {
                    let _ = mux.set_task_tab_description(&result, Some(description.clone()));
                } else {
                    let _ = mux.set_tab_description(&result, Some(description.clone()));
                }
            }
            if local_runtime && allocation.is_some() {
                let target = mux
                    .get_tab_mut(&result)
                    .ok_or("Cron runtime provisional tab absent")?;
                target.agent_type = Some(plugin_id.clone());
                target.backing = TabBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: String::new(),
                };
            }
            let original = spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.owner_linkage.as_ref())
                .and_then(|linkage| {
                    if !matches!(
                        linkage,
                        crate::service::spawn::RuntimeOwnerLinkage::CronRun { .. }
                    ) {
                        return None;
                    }
                    Some((mux.get_tab(&result)?.clone(), linkage.clone()))
                });
            (result, original)
        };
        let provisional = allocation
            .zip(original)
            .map(|(allocation, (expected, linkage))| {
                self.capture_allocated_cron_generation(&expected, &linkage)
                    .map(|owner| allocation.captured(owner))
            })
            .transpose()?;
        if let Some(provisional) = &provisional {
            provisional.prepare()?;
        }
        if let Some(attachment) = &team_attachment {
            let mut mux = self.mux.write();
            crate::service::team_membership::apply_team_attachment(&mut mux, attachment, &tab_id)?;
        }

        let launch = spec.chatbackend_launch.as_deref();

        let runtime_fork = launch.and_then(|launch| launch.runtime_fork.clone());

        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.backing = TabBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: String::new(),
                };
                tab.chat_view = true;
                tab.restorable_launch.execution_mode = config
                    .get("executionMode")
                    .and_then(|value| value.as_str())
                    .filter(|mode| *mode != "local")
                    .map(str::to_owned);
                tab.hidden = spec.visibility == super::spawn::TabVisibility::Hidden;
                tab.show_in_tabbar = !tab.hidden;
                if let Some(launch) = launch {
                    stamp_chatbackend_launch_identity(tab, launch);
                } else if local_runtime {
                    tab.agent_requested_model = config_model;
                }
                tab.audience = spec.audience;
                tab.role_profile = role_profile.clone();
                tab.taskless_authorized_gates = taskless_authorized_gates.clone();
                tab.watcher = watcher.clone();
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
        }
        if let Some(ref role) = role {
            let role_result = self.assign_chatbackend_spawn_role(
                &tab_id,
                role,
                role_profile.as_deref(),
                &spec,
                can_spawn,
                emit_state_change && runtime_fork.is_none(),
            );
            if let Err(e) = role_result {
                if provisional.is_none() {
                    self.rollback_provisional_tab_via_close(&tab_id);
                }
                if emit_state_change {
                    self.emit_state_changed(MuxEmitCause::TabLifecycle);
                }
                return Err(e);
            }
        }
        if role.is_none() && emit_state_change && runtime_fork.is_none() {
            self.emit_state_changed(MuxEmitCause::TabLifecycle);
        }

        if let Some(launch) = launch.filter(|launch| launch.params.manager_bootstrap.is_some()) {
            let mut params = launch.params.clone();
            params.system_prompt = config["systemPrompt"].as_str().map(str::to_owned);
            super::orchestrator::bind_manager_launch_identity(self, &tab_id, &mut params)
                .inspect_err(|_| {
                    if provisional.is_none() {
                        self.rollback_provisional_tab(&ws_id, &tab_id);
                    }
                })?;
            config["systemPrompt"] = serde_json::json!(params.system_prompt);
        }
        let history_meta = config.clone();

        if let Some(provisional) = &provisional {
            provisional.pad()?;
        } else {
            self.ensure_agent_pad(&tab_id)
                .inspect_err(|_| self.rollback_provisional_tab(&ws_id, &tab_id))?;
        }

        if cloud {
            if let Err(error) = self.persist_remote_launch() {
                if provisional.is_none() {
                    self.rollback_provisional_tab(&ws_id, &tab_id);
                    let _ = self.revoke_agent_pad(&tab_id);
                }
                return Err(error);
            }
        }
        if local_runtime {
            if let Some(launch) = launch {
                if let Err(error) = self.prepare_runtime_with_original(
                    &tab_id,
                    launch,
                    provisional.as_ref().map(|owner| owner.owner()),
                ) {
                    if !matches!(
                        launch.owner_linkage,
                        Some(super::spawn::RuntimeOwnerLinkage::CronRun { .. })
                    ) {
                        self.rollback_provisional_tab(&ws_id, &tab_id);
                        let _ = self.revoke_agent_pad(&tab_id);
                    }
                    return Err(error);
                }
            }
        }
        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        let handle = TabHandle {
            command_submission: None,
            tab_id: tab_id.clone(),
            pty_id: None,
            workspace_id: ws_id.clone(),
            description: spec.description,
            worker_id: launch.and_then(|launch| launch.agent.worker_id.clone()),
            warning,
            model_fallback: None,
            budget_notice,
        };
        Ok((
            handle,
            ChatBackendRuntimeLaunch {
                svc: self.clone(),
                plugin_id,
                tab_id,
                workspace_id: ws_id,
                config,
                history_meta,
                runtime_fork,
                emit_state_change,
                cron_owned: local_runtime
                    && launch.is_some_and(|metadata| {
                        matches!(
                            metadata.owner_linkage,
                            Some(super::spawn::RuntimeOwnerLinkage::CronRun { .. })
                        )
                    }),
                cron_owner: provisional.map(|owner| owner.transfer()),
            },
        ))
    }

    pub(crate) fn launch_chatbackend_runtime_existing(
        &self,
        tab_id: &str,
        metadata: &super::spawn::ChatBackendLaunchMetadata,
        config: serde_json::Value,
    ) -> Result<(), String> {
        if !super::agent_runtime::engine::requested(self, &metadata.agent.provider) {
            return Err("Existing-tab runtime launch requires agentRuntime".into());
        }
        self.prepare_chatbackend_runtime_launch(tab_id, metadata)?;
        let owner = metadata
            .owner_linkage
            .as_ref()
            .and_then(|linkage| match linkage {
                super::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id } => self
                    .run_watcher
                    .cron_cleanup_owners(job_id)
                    .into_iter()
                    .find(|owner| owner.run_id() == run_id && owner.tab_id() == tab_id),
                _ => None,
            });
        if matches!(
            metadata.owner_linkage,
            Some(super::spawn::RuntimeOwnerLinkage::CronRun { .. })
        ) && owner.is_none()
        {
            return Err("Original cron runtime owner is no longer retained".into());
        }
        let boundary = std::sync::atomic::AtomicBool::new(false);
        let result = self.chatbackend_launch_with_context(
            metadata.agent.provider.clone(),
            tab_id.into(),
            config.clone(),
            config,
            LaunchContext {
                boundary: Some(&boundary),
                cron_owner: owner.as_ref(),
                ..Default::default()
            },
        );
        if let (Err(error), Some(owner)) = (&result, owner) {
            if !boundary.load(std::sync::atomic::Ordering::Acquire) {
                let _ = self
                    .run_watcher
                    .fail_sealed_cron_launch(self, &owner, error);
            }
        }
        result
    }

    /// Opens and binds the provider session before spawn success is reported.
    /// Polling remains asynchronous after the tab has a durable session identity.
    pub(crate) fn chatbackend_launch(
        &self,
        plugin_id: String,
        tab_id: String,
        config: serde_json::Value,
        history_meta: serde_json::Value,
    ) -> Result<(), String> {
        self.chatbackend_launch_with_context(
            plugin_id,
            tab_id,
            config,
            history_meta,
            LaunchContext::default(),
        )
    }

    fn chatbackend_launch_with_context(
        &self,
        plugin_id: String,
        tab_id: String,
        config: serde_json::Value,
        history_meta: serde_json::Value,
        context: LaunchContext<'_>,
    ) -> Result<(), String> {
        if context
            .cron_owner
            .is_some_and(|owner| !owner.matches(&self.mux.read()))
        {
            return Err("Original cron runtime generation changed before configuration".into());
        }
        let cloud = self.remote_execution(&plugin_id, &config)?;
        if !cloud && super::agent_runtime::engine::requested(self, &plugin_id) {
            let args = self
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.restorable_launch.args.clone())
                .unwrap_or_default();
            super::spawn_capability::validate_runtime_launch_arguments(&plugin_id, &args)?;
            let config = {
                let mux = self.mux.read();
                let tab = mux.get_tab(&tab_id).ok_or("Runtime launch tab absent")?;
                launch_options::apply_prepared(
                    config,
                    &plugin_id,
                    tab.session_binding_nonce.as_deref(),
                    tab.restorable_launch.runtime_launch_options.as_ref(),
                )?
            };
            let mut config = config;
            let supports_fast = self
                .agent_registry
                .with_provider(&plugin_id, |provider| {
                    provider
                        .agent_runtime()
                        .is_some_and(|capability| !capability.service_tiers.is_empty())
                })
                .unwrap_or(false);
            let default_service_tier = self
                .configuration()
                .agents
                .get(&plugin_id)
                .and_then(|provider| provider.extra.get("default_service_tier"))
                .and_then(toml::Value::as_str)
                .map(
                    |id| codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
                        id: (!id.is_empty()).then(|| id.to_owned()),
                    },
                );
            launch_options::apply_service_tier_default(
                &mut config,
                supports_fast,
                default_service_tier,
            );
            return super::agent_runtime::transport::launch_with_context(
                self, &plugin_id, &tab_id, config, context,
            );
        }
        if context.fork.is_some() {
            return Err("Runtime fork requires an agent runtime capability".into());
        }
        let session_id = self
            .chatbackend_open_session(&plugin_id, &tab_id, config)
            .map_err(|error| {
                log::warn!(
                    "[spawn.chatbackend] openSession failed for {}: {}",
                    tab_id,
                    error
                );
                error
            })?;

        let bind_backing = {
            let mut mux = self.mux.write();
            match mux.get_tab_mut(&tab_id) {
                Some(tab) => match &mut tab.backing {
                    TabBacking::ChatBackend {
                        session_id: bound_session,
                        ..
                    } => {
                        *bound_session = session_id.clone();
                        Ok(())
                    }
                    _ => Err("chatBackend Tab backing changed during openSession".to_string()),
                },
                None => Err("chatBackend Tab disappeared during openSession".to_string()),
            }
        };
        if let Err(error) = bind_backing {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
            return Err(error);
        }
        if let Err(error) =
            self.bind_in_process_chatbackend_session(&tab_id, &plugin_id, &session_id)
        {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
            return Err(error);
        }
        self.emit_state_changed(MuxEmitCause::TabLifecycle);

        log::info!(
            "[spawn.chatbackend] Tab {} backed by plugin '{}' session '{}'",
            tab_id,
            plugin_id,
            session_id
        );
        let structural = self.classify_startup_nudge_for_hierarchy_report(&tab_id);
        self.observe_tab_state(
            &tab_id,
            crate::service::supporter_tab_state::ObservationTrigger::SpawnReady,
            structural.reason(),
        );

        let replay_tab = tab_id.clone();
        self.spawn_chatbackend_poll_loop(tab_id, plugin_id, session_id, history_meta);
        self.orchestration().schedule_planner_replay(&replay_tab);
        Ok(())
    }

    /// Background loop: poll the plugin and emit new transcript on
    /// `tab-chat-msg-{id}`, ending when the tab closes. Poll runs on a blocking
    /// thread so a slow plugin can't stall the async runtime.
    pub(crate) fn spawn_chatbackend_poll_loop(
        &self,
        tab_id: String,
        plugin_id: String,
        session_id: String,
        history_meta: serde_json::Value,
    ) {
        let svc = self.clone();
        spawn_bg(async move {
            let (source_session_id, session_binding_nonce) = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .map(|tab| {
                    (
                        tab.agent_session_id.clone(),
                        tab.session_binding_nonce.clone(),
                    )
                })
                .unwrap_or((None, None));
            let inner = crate::service::tab_chat::structured_sink::make_sink(
                svc.lifecycle_journal.clone(),
                &svc,
                svc.mux.clone(),
                source_session_id,
                session_binding_nonce,
                Some(svc.chat_send_ledger.clone()),
            );
            let history_plugin = plugin_id.clone();
            let history_session = session_id.clone();
            let sink: std::sync::Arc<
                dyn crate::service::tab_chat::structured_sink::StructuredSink,
            > = match tokio::task::spawn_blocking(move || {
                std::sync::Arc::new(
                    crate::service::tab_chat::structured_sink::ChatBackendStructuredSink::new(
                        inner,
                        &history_plugin,
                        &history_session,
                        Some(&history_meta),
                    ),
                )
            })
            .await
            {
                Ok(sink) => sink,
                Err(error) => {
                    log::warn!("[chatbackend] history initialization failed: {error}");
                    return;
                }
            };
            // None on the first tick ⇒ the plugin returns the whole transcript and seeds the cursor.
            let mut cursor: Option<String> = None;
            let mut delay_ms = POLL_IDLE_MS;
            let mut last_signature: Option<u64> = None;
            let mut watcher_cooldowns: HashMap<String, u64> = HashMap::new();
            let mut last_watcher_verdict_ids: HashMap<String, String> = HashMap::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                if svc.tab_chat_backing(&tab_id).as_ref()
                    != Some(&(plugin_id.clone(), session_id.clone()))
                {
                    break;
                }
                let (svc2, plugin2, sid2, cur) = (
                    svc.clone(),
                    plugin_id.clone(),
                    session_id.clone(),
                    cursor.clone(),
                );
                let tab_id_for_tick = tab_id.clone();
                let cooldowns = watcher_cooldowns.clone();
                let verdict_ids = last_watcher_verdict_ids.clone();
                let tick_sink = sink.clone();
                match tokio::task::spawn_blocking(
                    move || -> Result<
                        (PollTick, HashMap<String, u64>, HashMap<String, String>),
                        String,
                    > {
                        let mut cooldowns = cooldowns;
                        let mut verdict_ids = verdict_ids;
                        // Advance async work before reading, so a host-driven async chatBackend progresses.
                        svc2.chatbackend_pump(&plugin2, &sid2)?;
                        let delta = svc2.chatbackend_poll(&plugin2, &sid2, cur.as_deref())?;
                        let now_ms = epoch_ms();
                        svc2.clear_stale_watcher_inflight(&tab_id_for_tick, now_ms);
                        svc2.handle_watcher_verdict_delta(
                            &tab_id_for_tick,
                            &delta,
                            now_ms,
                            &mut cooldowns,
                            &mut verdict_ids,
                        );
                        svc2.handle_watcher_scheduler_tick(
                            &tab_id_for_tick,
                            &plugin2,
                            &sid2,
                            &delta,
                            now_ms,
                        );
                        let tick = plan_poll_tick(&delta, cur, delay_ms, last_signature);
                        if tick.emit {
                            tick_sink.emit(&tab_id_for_tick, delta.messages, None);
                        }
                        Ok((tick, cooldowns, verdict_ids))
                    },
                )
                .await
                {
                    Ok(Ok((tick, cooldowns, verdict_ids))) => {
                        watcher_cooldowns = cooldowns;
                        last_watcher_verdict_ids = verdict_ids;
                        cursor = tick.cursor;
                        delay_ms = tick.delay_ms;
                        last_signature = tick.signature;
                    }
                    Ok(Err(e)) => {
                        if let Ok(bound) = svc.chatbackend_bound_session(&plugin_id, &session_id) {
                            svc.chatbackend_liveness_failure(&plugin_id, &session_id, &bound, &e);
                        }
                        log::warn!("[chatbackend] poll Tab {} failed: {}", tab_id, e);
                        break;
                    }
                    Err(e) => {
                        log::warn!("[chatbackend] poll task join error: {}", e);
                        break;
                    }
                }
            }
            log::info!("[chatbackend] poll loop ended for Tab {}", tab_id);
        });
    }

    fn handle_watcher_scheduler_tick(
        &self,
        tab_id: &str,
        plugin_id: &str,
        session_id: &str,
        delta: &StructuredChat,
        now_ms: u64,
    ) {
        let plugin_busy = delta.done == Some(false);
        let Some((tick, state, observations_cfg)) =
            self.prepare_due_watcher_tick(tab_id, now_ms, plugin_busy)
        else {
            return;
        };
        let observations = self.build_watcher_observation(tab_id, &observations_cfg);
        let input = tick_input_json(tick, now_ms, state, observations);
        match self.chatbackend_watcher_tick(plugin_id, session_id, input) {
            Ok(()) => {
                self.mark_watcher_tick_inflight(tab_id, tick, now_ms);
            }
            Err(e) => {
                log::warn!("[watcher] tick enqueue failed for Tab {}: {}", tab_id, e);
                self.backoff_watcher_tick(tab_id, now_ms);
                self.emit_watcher_note(tab_id, format!("Watcher tick failed to enqueue: {e}"));
            }
        }
    }

    fn handle_watcher_verdict_delta(
        &self,
        tab_id: &str,
        delta: &StructuredChat,
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
        last_verdict_ids: &mut HashMap<String, String>,
    ) {
        for (id, raw) in extract_watcher_verdicts(delta) {
            if !accept_watcher_verdict_id(last_verdict_ids, tab_id, &id) {
                continue;
            }
            self.apply_watcher_verdict(tab_id, &id, &raw, now_ms, cooldowns);
        }
    }

    fn clear_stale_watcher_inflight(&self, tab_id: &str, now_ms: u64) -> bool {
        let mut cleared = false;
        {
            let mut mux = self.mux.write();
            let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) else {
                return false;
            };
            if clear_stale_watcher_inflight_spec(spec, now_ms) {
                cleared = true;
            }
        }
        if cleared {
            log::warn!("[watcher] stale inflight tick cleared for Tab {}", tab_id);
            self.emit_watcher_note(tab_id, "Watcher tick timed out; scheduling will resume.");
            self.emit_state_changed(MuxEmitCause::Watcher);
        }
        cleared
    }

    fn prepare_due_watcher_tick(
        &self,
        tab_id: &str,
        now_ms: u64,
        plugin_busy: bool,
    ) -> Option<(u64, serde_json::Value, serde_json::Value)> {
        let mux = self.mux.read();
        let spec = mux.get_tab(tab_id)?.watcher.as_ref()?;
        match plan_watcher_tick(spec, now_ms, plugin_busy) {
            WatcherTickPlan::Arm { tick } => {
                Some((tick, spec.state.clone(), spec.observations.clone()))
            }
            WatcherTickPlan::BlockedInflight | WatcherTickPlan::Wait => None,
        }
    }

    fn mark_watcher_tick_inflight(&self, tab_id: &str, tick: u64, now_ms: u64) {
        {
            let mut mux = self.mux.write();
            if let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) {
                // One poll loop owns a tab's watcher ticks; multiple loops would need an atomic claim.
                spec.tick = tick;
                spec.tick_inflight_since_ms = Some(now_ms);
            }
        }
        self.emit_state_changed(MuxEmitCause::Watcher);
    }

    fn backoff_watcher_tick(&self, tab_id: &str, now_ms: u64) {
        {
            let mut mux = self.mux.write();
            if let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) {
                spec.tick = spec.tick.saturating_add(1);
                spec.last_tick_ms = Some(now_ms);
                spec.tick_inflight_since_ms = None;
            }
        }
        self.emit_state_changed(MuxEmitCause::Watcher);
    }

    fn build_watcher_observation(
        &self,
        tab_id: &str,
        observations_cfg: &serde_json::Value,
    ) -> serde_json::Value {
        let config = observation_config_from_json(observations_cfg);
        let snapshot = self.snapshot_with_reconciled_status();
        let orchestrator_id = collect_watch_group(&snapshot, tab_id).map(|g| g.orchestrator_id);
        let reports = orchestrator_id
            .as_deref()
            .map(|id| self.report_store.lock().get(id, false))
            .unwrap_or_default();
        let last_activity_ms = HashMap::new();
        let chat_tail = |target: &str, n: usize| -> Vec<ChatMsg> {
            crate::service::tab_chat::tab_chat_snapshot(self, target, 0)
                .map(|(mut msgs, _)| {
                    let keep_from = msgs.len().saturating_sub(n);
                    msgs.drain(..keep_from);
                    msgs
                })
                .unwrap_or_default()
        };
        let sources = ObservationSources {
            chat_tail: &chat_tail,
            reports: &reports,
            last_activity_ms: &last_activity_ms,
        };
        build_observation(&snapshot, tab_id, &config, &sources)
    }

    fn apply_watcher_verdict(
        &self,
        tab_id: &str,
        verdict_id: &str,
        raw: &str,
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
    ) {
        let Some((policy, previous_state, group)) = self.watcher_guardrail_context(tab_id) else {
            log::warn!(
                "[watcher] dropping late verdict {} for Tab {} with no inflight tick",
                verdict_id,
                tab_id
            );
            self.emit_watcher_note(
                tab_id,
                format!("Dropped late watcher verdict {verdict_id}; no tick is in flight."),
            );
            return;
        };
        let verdict = parse_verdict(raw);
        let outcome = apply_guardrails(
            &verdict,
            &policy,
            &group,
            cooldowns,
            now_ms,
            &previous_state,
        );
        for (dropped, reason) in &outcome.dropped {
            log::warn!(
                "[watcher] Tab {} dropped {} action to {:?}: {}",
                tab_id,
                dropped.kind,
                dropped.tab,
                reason
            );
        }
        if !outcome.dropped.is_empty() {
            let dropped_summary = outcome
                .dropped
                .iter()
                .map(|(a, r)| format!("{} -> {:?} ({r})", a.kind, a.tab))
                .collect::<Vec<_>>()
                .join(", ");
            self.emit_watcher_note(
                tab_id,
                format!("Guardrails dropped actions: {dropped_summary}"),
            );
        }
        let actions = outcome.allowed_actions.clone();
        let applied = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .and_then(|p| p.watcher.as_mut())
                .map(|spec| apply_watcher_outcome_to_spec(spec, outcome, now_ms))
                .unwrap_or(false)
        };
        if applied {
            self.execute_watcher_actions(tab_id, &actions, now_ms, cooldowns);
            self.emit_state_changed(MuxEmitCause::Watcher);
        } else {
            log::warn!(
                "[watcher] dropping late verdict {} for Tab {} with no inflight tick",
                verdict_id,
                tab_id
            );
            self.emit_watcher_note(
                tab_id,
                format!("Dropped late watcher verdict {verdict_id}; no tick is in flight."),
            );
        }
    }

    fn watcher_guardrail_context(
        &self,
        tab_id: &str,
    ) -> Option<(WatcherPolicy, serde_json::Value, HashSet<String>)> {
        let snapshot = self.snapshot_with_reconciled_status();
        let spec = snapshot.tabs.get(tab_id)?.watcher.as_ref()?;
        spec.tick_inflight_since_ms?;
        let group = guardrail_group(collect_watch_group(&snapshot, tab_id).ok()?);
        Some((spec.policy.clone(), spec.state.clone(), group))
    }

    fn execute_watcher_actions(
        &self,
        tab_id: &str,
        actions: &[WatcherAction],
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
    ) {
        execute_watcher_actions_with(
            tab_id,
            actions,
            now_ms,
            cooldowns,
            |target, message| {
                let from_agent = self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting);
                let envelope = MessageEnvelope::tab("Watcher", tab_id).from_agent(Some(from_agent));
                self.gateway_deliver(
                    target,
                    envelope,
                    message,
                    crate::service::message_gateway::DeliveryClass::AgentTraffic,
                )
                .map(|_| {
                    self.emit_watcher_note(
                        tab_id,
                        format!("Nudge delivered to {target}: {message}"),
                    );
                })
            },
            |message| {
                self.log_system_notice(
                    "Watcher notification".to_string(),
                    Some(message.to_string()),
                    Some("warning"),
                );
            },
            |message| {
                self.orchestration()
                    .submit_report(tab_id, message, ReportDetails::default(), None, None, None)
                    .map(|_| ())
                    .map_err(|error| error.message)
            },
        );
    }

    fn emit_watcher_note(&self, tab_id: &str, text: impl Into<String>) {
        let note = watcher_system_note(text);
        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            crate::service::tab_chat::chatbackend_history::ChatBackendHistory::shared(
                &plugin_id,
                &session_id,
            )
            .lock()
            .upsert(std::slice::from_ref(&note));
        }
        let (source_session_id, session_binding_nonce) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                )
            })
            .unwrap_or((None, None));
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            self.ui.as_ref(),
            tab_id,
            &[note],
            None,
            source_session_id.as_deref(),
            session_binding_nonce.as_deref(),
        );
    }
}

#[cfg(test)]
mod tests;
