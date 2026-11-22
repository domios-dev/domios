use super::super::ServiceContext;
use super::*;
use crate::agent::provider::SystemPromptDelivery;
use crate::agent::run_watcher::runtime_runs::CronRunAuthority;
use crate::service::MuxEmitCause;

/// What the first runtime turn carries: a system-prompt-capable provider already
/// holds the brief in its instructions, so it only receives the assigned-task starter.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum RuntimeFirstTurn {
    Starter(String),
    Task(String),
}

pub(super) fn runtime_pending_payload(
    existing: Option<&crate::multiplexer::types::PendingDeliveryRecord>,
    agent: &crate::service::spawn::AgentSpawnSpec,
    provider: Option<&dyn crate::agent::provider::AgentProvider>,
    system_prompt_delivery: Option<&SystemPromptDelivery>,
) -> Option<RuntimeFirstTurn> {
    if existing.is_some() {
        None
    } else {
        runtime_initial_payload(agent, provider, system_prompt_delivery)
    }
}

pub(super) fn runtime_initial_payload(
    agent: &crate::service::spawn::AgentSpawnSpec,
    provider: Option<&dyn crate::agent::provider::AgentProvider>,
    system_prompt_delivery: Option<&SystemPromptDelivery>,
) -> Option<RuntimeFirstTurn> {
    let task = agent
        .task
        .as_deref()
        .filter(|task| !task.trim().is_empty())?;
    let delivery = system_prompt_delivery
        .cloned()
        .or_else(|| provider.map(|provider| provider.system_prompt_delivery()));
    let instructions_capable = delivery.as_ref().is_some_and(|delivery| {
        matches!(
            crate::service::launch_delivery::mode_for_descriptor(delivery),
            crate::service::launch_delivery::LaunchDeliveryMode::SystemPrompt
        )
    });
    if instructions_capable {
        let prompt = crate::agent::provider::StarterPrompt::AssignedTask;
        let text = crate::service::launch_delivery::assigned_task_starter(
            provider
                .and_then(|provider| provider.starter_prompt_text(prompt))
                .as_deref(),
        );
        return Some(RuntimeFirstTurn::Starter(
            crate::service::launch_delivery::mark_starter_prompt(prompt, text),
        ));
    }
    Some(RuntimeFirstTurn::Task(
        match agent.task_preamble.as_deref() {
            Some(preamble) => format!("{preamble}{task}"),
            None => task.into(),
        },
    ))
}

/// The provider runtime start of a staged chatBackend tab, which a dispatch may run off its RPC thread.
pub(crate) struct ChatBackendRuntimeLaunch {
    pub(super) svc: ServiceContext,
    pub(super) plugin_id: String,
    pub(super) tab_id: String,
    pub(super) workspace_id: String,
    pub(super) config: serde_json::Value,
    pub(super) history_meta: serde_json::Value,
    pub(super) runtime_fork: Option<crate::service::spawn::RuntimeForkLaunch>,
    pub(super) emit_state_change: bool,
    pub(super) cron_owned: bool,
    pub(super) cron_owner: Option<CronRunAuthority>,
}

impl ChatBackendRuntimeLaunch {
    pub(crate) fn can_defer(&self) -> bool {
        self.runtime_fork.is_none()
    }

    pub(crate) fn run(self) -> Result<(), String> {
        let svc = self.svc;
        let runtime_boundary = std::sync::atomic::AtomicBool::new(false);
        if let Err(error) = svc.chatbackend_launch_with_runtime_fork(
            self.plugin_id,
            self.tab_id.clone(),
            self.config,
            self.history_meta,
            self.runtime_fork.clone(),
            Some(&runtime_boundary),
            self.cron_owner.as_ref(),
        ) {
            if let Some(owner) = &self.cron_owner {
                if self.cron_owned && !runtime_boundary.load(std::sync::atomic::Ordering::Acquire) {
                    let _ = svc.run_watcher.fail_sealed_cron_launch(&svc, owner, &error);
                }
            }
            if self.cron_owner.is_none()
                && !self.cron_owned
                && (self.runtime_fork.is_some()
                    || !crate::service::agent_runtime::requested_for_tab(&svc, &self.tab_id))
            {
                svc.rollback_provisional_tab(&self.workspace_id, &self.tab_id);
                let _ = svc.revoke_agent_pad(&self.tab_id);
            }
            if self.emit_state_change {
                svc.emit_state_changed(MuxEmitCause::TabLifecycle);
            }
            return Err(error);
        }
        svc.record_kpi_tab_created(
            &self.tab_id,
            crate::agent::kpi_ledger::TabCreatedKind::Agent,
        );
        Ok(())
    }
}

impl ServiceContext {
    pub(super) fn runtime_system_prompt_delivery(
        &self,
        provider_id: &str,
        provider: Option<&dyn crate::agent::provider::AgentProvider>,
    ) -> Option<SystemPromptDelivery> {
        provider
            .map(|provider| provider.system_prompt_delivery())
            .filter(|delivery| !matches!(delivery, SystemPromptDelivery::External))
            .or_else(|| {
                self.chatbackend_registry
                    .descriptor(provider_id)
                    .map(|descriptor| descriptor.system_prompt_delivery)
            })
    }

    pub(crate) fn prepare_chatbackend_runtime_launch(
        &self,
        tab: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
    ) -> Result<(), String> {
        let provisional = if let Some(
            linkage @ crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id },
        ) = metadata.owner_linkage.as_ref()
        {
            let cron = self
                .try_ext::<crate::CronState>()
                .ok_or("Cron state unavailable before runtime allocation")?;
            self.run_watcher
                .reserve_cron_slot(self, cron, job_id, run_id)?;
            let allocation =
                crate::service::spawn::cron_launch::CronAllocation::claim(self, job_id, run_id)?;
            let expected = self
                .mux
                .read()
                .get_tab(tab)
                .cloned()
                .ok_or("Runtime provisional tab absent")?;
            let owner = self.capture_allocated_cron_generation(&expected, linkage)?;
            Some(allocation.captured(owner))
        } else {
            None
        };
        if let Some(provisional) = &provisional {
            provisional.prepare()?;
        }
        self.prepare_runtime_with_original(
            tab,
            metadata,
            provisional.as_ref().map(|owner| owner.owner()),
        )?;
        if let Some(provisional) = provisional {
            provisional.transfer();
        }
        Ok(())
    }

    pub(super) fn prepare_runtime_with_original(
        &self,
        tab_id: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
        original_cron: Option<&CronRunAuthority>,
    ) -> Result<(), String> {
        if original_cron.is_some_and(|owner| !owner.matches(&self.mux.read())) {
            return Err("Original runtime generation changed before preparation".into());
        }
        crate::service::spawn_capability::validate_runtime_launch_arguments(
            &metadata.agent.provider,
            &metadata.agent.extra_args,
        )?;
        let params =
            launch_options::authorize(self, &metadata.agent.provider, Some(&metadata.params))?;
        log::debug!(
            "[spawn.runtime] prepared tab={tab_id} plan={:?} task={:?} intent={:?} manifest_entries={}",
            params.plan_id,
            params.task_id,
            params.execution_intent,
            params.plan_skill_manifest.as_ref().map_or(0, Vec::len)
        );
        if let Some(owner) = original_cron {
            if self.run_watcher.cron_pad_lease(owner).is_none() {
                self.capture_cron_pad(owner)?;
            }
        }
        self.ensure_agent_pad(tab_id)?;
        self.ensure_session_binding_nonce(tab_id);
        {
            let mut mux = self.mux.write();
            if original_cron.is_some_and(|owner| !owner.matches(&mux)) {
                return Err("Runtime cron launch changed during preparation".into());
            }
            let tab = mux.get_tab_mut(tab_id).ok_or("Runtime launch tab absent")?;
            if tab.pty_id.is_some() {
                return Err("Existing runtime launch requires released PTY ownership".into());
            }
            if metadata.agent.fork_session {
                return Err("Runtime spawn cannot replace a bound thread with a fork".into());
            }
            tab.agent_session_id = runtime_resume_thread(
                tab.agent_session_id.as_deref(),
                metadata.agent.resume_session_id.as_deref(),
            )?;
            if let Some(cwd) = metadata.params.cwd.as_deref() {
                tab.cwd = Some(Self::normalize_resolved_agent_cwd(cwd));
            }
            tab.backing = TabBacking::ChatBackend {
                plugin_id: metadata.agent.provider.clone(),
                session_id: tab.agent_session_id.clone().unwrap_or_default(),
            };
            tab.chat_view = true;
            stamp_chatbackend_launch_identity(tab, metadata);
        }
        if let Some(worker_id) = metadata.agent.worker_id.as_deref() {
            self.workers.set_tab_id(worker_id, tab_id.into());
        }
        if let Some(crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) =
            &metadata.owner_linkage
        {
            if let Some(owner) = original_cron {
                let cron = self
                    .try_ext::<crate::CronState>()
                    .ok_or("Cron state unavailable before runtime admission")?;
                self.reserve_captured_cron_launch(cron, owner)?;
                self.bind_cron_spawn(
                    cron,
                    tab_id,
                    job_id,
                    run_id,
                    owner.nonce().map(str::to_string),
                    None,
                )?;
            } else {
                return Err("Runtime cron launch has no original generation".into());
            }
        }
        self.prepare_runtime_run_watcher(tab_id, metadata)?;
        self.prepare_runtime_worker_binding(&params, tab_id)?;
        self.prepare_agent_runtime_launch_options(tab_id, &params)?;
        self.prepare_agent_runtime_launch_env(tab_id, &metadata.agent.extra_env)?;
        let provider = self.agent_registry.provider_arc(&metadata.agent.provider);
        let delivery =
            self.runtime_system_prompt_delivery(&metadata.agent.provider, provider.as_deref());
        let payload = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("Runtime launch tab absent")?;
            runtime_pending_payload(
                tab.pending_delivery.as_ref(),
                &metadata.agent,
                provider.as_deref(),
                delivery.as_ref(),
            )
        };
        if let Some(payload) = payload {
            let payload = match payload {
                RuntimeFirstTurn::Starter(starter) => starter,
                RuntimeFirstTurn::Task(task) => {
                    self.spawn_task_message(tab_id, &task).unwrap_or(task)
                }
            };
            let send = uuid::Uuid::new_v4().to_string();
            if let Some(owner) = original_cron {
                use sha2::Digest;
                let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));
                let record = crate::multiplexer::types::PendingDeliveryRecord::new(
                    digest,
                    crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                )
                .with_send_id(send);
                self.pending_delivery_payloads
                    .write(&record, &payload)
                    .map_err(|error| error.to_string())?;
                if let Err(error) =
                    self.run_watcher
                        .seal_prepared_cron_input(self, owner, Some(record.clone()))
                {
                    let live = crate::service::pending_delivery_payloads::live_payload_keys(
                        &self.mux.read().snapshot(),
                    );
                    if let Err(cleanup) = self.pending_delivery_payloads.delete(&record, &live) {
                        return Err(format!(
                            "{error}; original candidate payload cleanup refused: {cleanup}"
                        ));
                    }
                    return Err(error);
                }
                crate::service::session_persist::persist_session_after_mutation(
                    self,
                    crate::service::session_persist::SessionPersistUrgency::Prompt,
                )?;
            } else {
                self.create_pending_delivery_with_send_id(
                    tab_id,
                    crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                    &payload,
                    Some(&send),
                )
                .map_err(|error| error.to_string())?;
            }
        } else if metadata
            .agent
            .task
            .as_deref()
            .is_none_or(|task| task.trim().is_empty())
        {
            if let Some(owner) = original_cron {
                if self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .is_some_and(|tab| tab.pending_delivery.is_none())
                {
                    self.run_watcher
                        .seal_prepared_cron_input(self, owner, None)?;
                }
            }
        }
        Ok(())
    }
}
