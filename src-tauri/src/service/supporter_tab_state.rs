use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::supporter::{self, Action, ActionRecord, SupporterRuntime, TokenSnapshot, VerdictKind};
use codeterm_types::config::SupporterConfig;
use codeterm_types::decision::{
    DecisionAnswer, DecisionModel, DecisionQuestionsRequest, DecisionResponse,
};
use serde_json::Value;

#[cfg(test)]
static TEST_SUPPORTER_OVERRIDES: std::sync::LazyLock<
    parking_lot::Mutex<HashMap<usize, (SupporterConfig, Arc<SupporterRuntime>)>>,
> = std::sync::LazyLock::new(Default::default);

#[cfg(test)]
pub(crate) struct TestSupporterOverrideGuard(usize);

#[cfg(test)]
impl Drop for TestSupporterOverrideGuard {
    fn drop(&mut self) {
        TEST_SUPPORTER_OVERRIDES.lock().remove(&self.0);
    }
}

use super::startup_nudge::StartupNudge;
use super::tab_transport::TabTransport;
use super::ServiceContext;
use crate::api::mem_routes::decision::SupporterRegistry;

mod candidates;
pub(crate) mod coordinator;
pub(crate) use candidates::{allows_prompt as candidate_allows_prompt, CandidateVerdict};
mod delivery;
mod identity;
mod observer;
mod privacy;
pub(super) mod scheduler;

pub(crate) use identity::correlation::project_diagnostics as project_observation_diagnostics;
#[cfg(test)]
use observer::{choice_model, observation_answers, observe_guarded};
pub(crate) use observer::{prepare_observation, spawn_observer};
pub(crate) use privacy::value as redact_decision_diagnostic;
mod questions;
pub(crate) mod safe_choice;
mod safe_response;
pub(crate) use safe_response::Progress as SafeResponseProgress;
mod state;

pub(crate) use state::{
    apply_observed_verdict, claim_observation, confidence_band, launch_episode_key, note_output,
    permit_is_current_for_launch, ConfidenceBand, ObservationEvent, SignalAction,
};

pub(crate) const SCREEN_LINES: usize = 40;
pub(crate) const DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const INSTRUCTIONS: &str =
    "Classify the current agent tab state using the screen and runtime context. Choose the single best label.";

pub(crate) const READY_IDLE: &str = "ready_idle";
pub(crate) const WORKING_NORMALLY: &str = "working_normally";
pub(crate) const PROVIDER_UPDATE_PROMPT: &str = "provider_update_prompt";
pub(crate) const TRUST_OR_PERMISSION_PROMPT: &str = "trust_or_permission_prompt";
pub(crate) const LOGIN_OR_AUTH_REQUIRED: &str = "login_or_auth_required";
pub(crate) const MODEL_UNAVAILABLE_OR_QUOTA: &str = "model_unavailable_or_quota";
pub(crate) const CRASHED_TO_SHELL: &str = "crashed_to_shell";
pub(crate) const ERROR_LOOP: &str = "error_loop";
pub(crate) const OTHER: &str = "other";
pub(crate) const STILL_STARTING: &str = "still_starting";

const CRITERIA_JSON: &str = include_str!("../../../evals/tab-state-jev/criteria.json");
const SPAWN_OK_LINE: &str = "spawn ok";
const WEAK_HINT_PREFIX: &str = "Tab-state hint";

pub(crate) fn criteria_version() -> String {
    supporter::criteria_hash(CRITERIA_JSON.as_bytes())
}

pub(crate) fn criteria() -> &'static BTreeMap<String, String> {
    static CRITERIA: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CRITERIA.get_or_init(|| {
        serde_json::from_str(CRITERIA_JSON).expect("embedded tab-state criteria are valid")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationTrigger {
    SpawnReady,
    SpawnDeadline,
    StartupEscalation,
    PromptCandidate,
    OutputCandidate,
    ParkedEdge,
    PromptEstablished,
    PromptConfirmation,
    WorkerIdle,
    BusyNoOutput,
    TabExit,
}

impl ObservationTrigger {
    fn event(self) -> ObservationEvent {
        match self {
            Self::SpawnReady => ObservationEvent::SpawnReady,
            Self::SpawnDeadline => ObservationEvent::SpawnDeadline,
            Self::StartupEscalation => ObservationEvent::StartupEscalation,
            Self::PromptCandidate
            | Self::OutputCandidate
            | Self::ParkedEdge
            | Self::PromptEstablished
            | Self::PromptConfirmation => ObservationEvent::PromptCandidate,
            Self::WorkerIdle => ObservationEvent::WorkerIdle,
            Self::BusyNoOutput => ObservationEvent::BusyNoOutput,
            Self::TabExit => ObservationEvent::TabExit,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SpawnReady => "launch_ready",
            Self::SpawnDeadline => "startup_deadline",
            Self::StartupEscalation => "startup_escalation",
            Self::PromptCandidate
            | Self::OutputCandidate
            | Self::ParkedEdge
            | Self::PromptEstablished
            | Self::PromptConfirmation => "prompt_candidate",
            Self::WorkerIdle => "worker_idle_without_report",
            Self::BusyNoOutput => "busy_without_output",
            Self::TabExit => "tab_exit",
        }
    }

    fn is_spawn(self) -> bool {
        matches!(self, Self::SpawnReady | Self::SpawnDeadline)
    }

    fn producer(self) -> identity::correlation::Producer {
        use identity::correlation::Producer;
        match self {
            Self::OutputCandidate => Producer::OutputCandidate,
            Self::ParkedEdge => Producer::ParkedEdge,
            Self::PromptEstablished => Producer::PromptEstablished,
            Self::PromptConfirmation => Producer::PromptConfirmation,
            _ => Producer::LifecycleEvent,
        }
    }

    fn is_prompt_candidate(self) -> bool {
        matches!(
            self,
            Self::PromptCandidate
                | Self::OutputCandidate
                | Self::ParkedEdge
                | Self::PromptEstablished
                | Self::PromptConfirmation
        )
    }

    fn is_internal_candidate(self) -> bool {
        matches!(self, Self::PromptEstablished | Self::PromptConfirmation)
    }

    fn after_startup_deadline(self) -> bool {
        matches!(self, Self::SpawnDeadline)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NumberedPromptOption {
    pub(crate) number: usize,
    pub(crate) label: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TabStateMetadata {
    pub(crate) context: Value,
    pub(crate) session_key: String,
    pub(crate) title: String,
    pub(crate) spawning_tab_id: Option<String>,
    pub(crate) parent_tab_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) attempt_id: Option<String>,
    pub(crate) prompt_options: Vec<NumberedPromptOption>,
    pub(crate) declared_safe_choice: Option<String>,
    pub(crate) tab_snapshot: Option<crate::multiplexer::types::Tab>,
    pub(crate) process_tree_key: Option<super::process_tree::ProviderTreeKey>,
    pub(crate) envelope: Option<observer::EnvelopeContext>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShadowVerdict {
    pub(crate) label: String,
    pub(crate) confidence: f64,
    pub(crate) prompt_visible: f64,
    pub(crate) safe_choice: Option<(String, f64)>,
    pub(crate) action: SignalAction,
    pub(crate) classified_frame_hash: u64,
}

pub(crate) trait ShadowHost: Clone + Send + 'static {
    fn local_identity(&self, _tab: &str) -> Option<identity::LocalIdentity> {
        None
    }
    fn ledger_snapshot(&self, _tab: &str) -> coordinator::LedgerSnapshot {
        Default::default()
    }
    fn observation_is_current(&self, _tab: &str, _metadata: &TabStateMetadata) -> bool {
        true
    }
    fn connection_is_current(&self, adapter: &str) -> bool {
        self.supporter_models().get(adapter).is_some_and(|model| {
            model.token_mode() != codeterm_types::decision::TokenMode::Required
                || self.token_available(adapter)
        })
    }

    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String>;
    fn supporter_models(&self) -> Arc<SupporterRegistry>;
    fn token_available(&self, adapter_id: &str) -> bool;
    fn supporter_runtime(&self) -> Arc<SupporterRuntime> {
        supporter::runtime()
    }
    fn metadata(
        &self,
        tab_id: &str,
        trigger: ObservationTrigger,
        structural_verdict: &str,
        screen: &str,
    ) -> Result<TabStateMetadata, String>;
    fn deliver(
        &self,
        tab_id: &str,
        metadata: &TabStateMetadata,
        episode: u64,
        trigger: ObservationTrigger,
        label: &str,
        confidence: f64,
        action: SignalAction,
    );
    fn recover(
        &self,
        _tab_id: &str,
        _metadata: &TabStateMetadata,
        _episode: u64,
        _observation_id: u64,
        _trigger: ObservationTrigger,
        _verdict: &ShadowVerdict,
        _ticket: Option<&coordinator::ActionTicket>,
    ) -> bool {
        false
    }
    fn restore_observation(
        &self,
        _tab_id: &str,
    ) -> Option<super::supporter_notify::RestoreObservation> {
        None
    }
    fn record(&self, line: &str);
}

impl ShadowHost for ServiceContext {
    fn local_identity(&self, tab: &str) -> Option<identity::LocalIdentity> {
        self.mux
            .read()
            .get_tab(tab)
            .map(identity::LocalIdentity::of_tab)
    }
    fn ledger_snapshot(&self, tab: &str) -> coordinator::LedgerSnapshot {
        coordinator::LedgerSnapshot::capture(self, tab)
    }
    fn connection_is_current(&self, adapter: &str) -> bool {
        let settings = self.supporter_decision_config();
        settings.active_id() == Some(adapter)
            && supporter::supporter_connected(
                &settings,
                &self.supporter_models,
                &TokenSnapshot::read(&crate::api::mem_routes::decision::token_store(), adapter),
            )
    }
    fn observation_is_current(&self, tab: &str, metadata: &TabStateMetadata) -> bool {
        let Some(envelope) = metadata.envelope.as_ref() else {
            return false;
        };
        if envelope.policy_hash.as_ref().is_some_and(|expected| {
            identity::supporter_policy(&self.supporter_decision_config()) != *expected
        }) {
            return false;
        }
        let Some(local) = envelope.local.as_ref() else {
            return false;
        };
        if !self.observation_identity_is_current(tab, local) {
            return false;
        }
        if metadata
            .tab_snapshot
            .as_ref()
            .and_then(|tab| tab.pty_id.as_deref())
            .is_some_and(|pty| self.pty.lock().get_session(pty).is_none())
        {
            return false;
        }
        let frame = self
            .tab_transport(tab)
            .and_then(|transport| transport.read_screen(false));
        frame.is_ok_and(|frame| frame_hash(&frame.to_string()) == envelope.frame_hash)
            && self.observation_identity_is_current(tab, local)
    }

    fn supporter_runtime(&self) -> Arc<SupporterRuntime> {
        #[cfg(test)]
        if let Some((_, runtime)) = TEST_SUPPORTER_OVERRIDES
            .lock()
            .get(&(Arc::as_ptr(&self.mux) as usize))
        {
            return runtime.clone();
        }
        supporter::runtime_with_notifications(self)
    }
    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        self.tab_transport(tab_id)
    }

    fn supporter_models(&self) -> Arc<SupporterRegistry> {
        self.supporter_models.clone()
    }

    fn token_available(&self, adapter_id: &str) -> bool {
        TokenSnapshot::read(&crate::api::mem_routes::decision::token_store(), adapter_id).usable
    }

    fn metadata(
        &self,
        tab_id: &str,
        trigger: ObservationTrigger,
        structural_verdict: &str,
        screen: &str,
    ) -> Result<TabStateMetadata, String> {
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or_else(|| format!("tab {tab_id} disappeared before observation"))?;
        let provider_id = tab
            .agent_type
            .as_deref()
            .ok_or_else(|| "tab has no provider identity for capability lookup".to_string())?;
        let descriptor = self
            .provider_registry
            .describe_all()
            .into_iter()
            .find(|descriptor| descriptor.id == provider_id)
            .ok_or_else(|| "provider capability descriptor is unavailable".to_string())?;
        let transport = match descriptor.transport {
            crate::agent::provider::ProviderTransport::Pty => "pty",
            crate::agent::provider::ProviderTransport::ChatBackend => "chat_backend",
        };
        let now = Instant::now();
        let seconds_since_spawn = tab
            .agent_set_at
            .map(|started| now.saturating_duration_since(started).as_secs())
            .unwrap_or_default();
        let local_identity = identity::LocalIdentity::of_tab(&tab);
        let session_key = local_identity.launch_key.clone();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        screen.hash(&mut hasher);
        let changed = {
            let mux = self.mux.read();
            if !mux
                .get_tab(&tab.id)
                .is_some_and(|current| identity::LocalIdentity::of_tab(current) == local_identity)
            {
                return Err("stale observation capture".into());
            }
            state::screen_changed(&tab.id, &session_key, hasher.finish())
        };
        let task_binding = tab.task_binding.as_ref();
        let seconds_since_last_output = tab
            .pty_id
            .as_deref()
            .and_then(|pty_id| self.agent_tracker.lock().quiescence(pty_id, now))
            .map(|(byte_idle_ms, _)| byte_idle_ms / 1000)
            .unwrap_or_else(|| {
                if changed {
                    0
                } else {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let last_output = state::last_output_epoch_seconds(&tab.id);
                    if last_output == 0 {
                        seconds_since_spawn
                    } else {
                        now_epoch.saturating_sub(last_output)
                    }
                }
            });
        let declared_safe_choice = tab
            .pending_prompt
            .as_ref()
            .filter(|_| {
                tab.pending_prompt_kind == Some(codeterm_types::pty::PromptKind::ManualUpdate)
            })
            .and_then(|prompt| {
                self.agent_registry
                    .with_provider(provider_id, |provider| {
                        provider.update_prompt_safe_choice(prompt)
                    })
                    .flatten()
            });
        let context = serde_json::json!({
            "provider_capability": {
                "transport": transport,
                "display_name": &descriptor.display_name,
            },
            "requested_model": tab.agent_requested_model.as_deref(),
            "session_model": tab.agent_session_model.as_deref(),
            "role": tab.role,
            "role_profile": tab.role_profile.as_deref(),
            "task_binding": {
                "plan_id": task_binding.and_then(|binding| binding.plan_id.as_deref()),
                "task_id": task_binding.and_then(|binding| binding.task_id.as_deref()),
                "attempt_id": task_binding.and_then(|binding| binding.attempt_id.as_deref()),
            },
            "plan_id": task_binding.and_then(|binding| binding.plan_id.as_deref()),
            "task_id": task_binding.and_then(|binding| binding.task_id.as_deref()),
            "spawning_tab_id": tab.spawned_by_tab_id.as_deref(),
            "seconds_since_spawn": seconds_since_spawn,
            "seconds_since_last_output": seconds_since_last_output,
            "structural_verdict": {
                "event_detector": structural_verdict,
                "agent_state": tab.agent_state,
            },
            "pending_prompt_kind": tab.pending_prompt_kind,
            "startup_deadline_elapsed": trigger.after_startup_deadline() || tab.boot_stage == Some(codeterm_types::pty::BootStage::LaunchDeadlineElapsed),
            "screen_changed": changed,
            "observation_event": trigger.label(),
        });
        let prompt_options = tab
            .pending_prompt
            .as_ref()
            .map(|prompt| {
                prompt
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| NumberedPromptOption {
                        number: index + 1,
                        label: option.label.clone(),
                        value: option.value.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let process_tree_key = tab
            .pty_id
            .as_deref()
            .and_then(|pty| self.provider_tree_key_for_tab(&tab.id, pty));
        let tab_snapshot = Some(tab.clone());
        let mut metadata = TabStateMetadata {
            envelope: None,
            declared_safe_choice,
            tab_snapshot,
            process_tree_key,
            context,
            session_key,
            title: tab.title,
            spawning_tab_id: tab.spawned_by_tab_id,
            parent_tab_id: tab.parent_tab_id,
            plan_id: task_binding.and_then(|binding| binding.plan_id.clone()),
            task_id: task_binding.and_then(|binding| binding.task_id.clone()),
            attempt_id: task_binding.and_then(|binding| binding.attempt_id.clone()),
            prompt_options,
        };
        if trigger.is_prompt_candidate() {
            self.add_safe_response_metadata(tab_id, screen, &mut metadata);
        }
        Ok(metadata)
    }

    fn deliver(
        &self,
        tab_id: &str,
        metadata: &TabStateMetadata,
        episode: u64,
        trigger: ObservationTrigger,
        label: &str,
        confidence: f64,
        action: SignalAction,
    ) {
        delivery::deliver(
            self, tab_id, metadata, episode, trigger, label, confidence, action,
        );
    }

    fn recover(
        &self,
        tab_id: &str,
        metadata: &TabStateMetadata,
        episode: u64,
        observation_id: u64,
        trigger: ObservationTrigger,
        verdict: &ShadowVerdict,
        ticket: Option<&coordinator::ActionTicket>,
    ) -> bool {
        super::general_agent::recovery::recover(
            self,
            tab_id,
            metadata,
            episode,
            observation_id,
            trigger,
            verdict,
            ticket,
        )
    }

    fn restore_observation(
        &self,
        tab_id: &str,
    ) -> Option<super::supporter_notify::RestoreObservation> {
        super::supporter_notify::begin_restore_observation(self, tab_id)
    }

    fn record(&self, line: &str) {
        log::info!("{line}");
    }
}

impl ServiceContext {
    pub(crate) fn supporter_decision_config(&self) -> SupporterConfig {
        #[cfg(test)]
        if let Some((config, _)) = TEST_SUPPORTER_OVERRIDES
            .lock()
            .get(&(Arc::as_ptr(&self.mux) as usize))
        {
            return config.clone();
        }
        crate::config::load_config().supporter
    }

    #[cfg(test)]
    pub(crate) fn override_supporter_decision_for_test(
        &self,
        config: SupporterConfig,
        runtime: Arc<SupporterRuntime>,
    ) -> TestSupporterOverrideGuard {
        let key = Arc::as_ptr(&self.mux) as usize;
        TEST_SUPPORTER_OVERRIDES
            .lock()
            .insert(key, (config, runtime));
        TestSupporterOverrideGuard(key)
    }

    pub(crate) fn observe_launch_settlement_tab_state(
        &self,
        tab_id: &str,
        settlement: &crate::service::launch_readiness::LaunchSettlement,
    ) {
        let ga_tab = { self.mux.read().get_tab(tab_id).cloned() };
        if let Some(tab) = ga_tab {
            self.general_agent_watchdog_event(
                &tab,
                super::general_agent::WatchdogEvent::LaunchSettlement,
            );
        }
        let trigger = match settlement {
            crate::service::launch_readiness::LaunchSettlement::Ready => {
                ObservationTrigger::SpawnReady
            }
            crate::service::launch_readiness::LaunchSettlement::Provisional { stage, .. }
                if *stage == codeterm_types::pty::BootStage::LaunchDeadlineElapsed =>
            {
                ObservationTrigger::SpawnDeadline
            }
            _ => return,
        };
        let structural = self.classify_startup_nudge_for_hierarchy_report(tab_id);
        self.observe_tab_state(tab_id, trigger, structural.reason());
    }

    pub(crate) fn observe_tab_state(
        &self,
        tab_id: &str,
        trigger: ObservationTrigger,
        structural_verdict: &str,
    ) {
        let settings = self.supporter_decision_config();
        let _ = spawn_observer(
            self,
            &settings,
            settings.active_id().unwrap_or(""),
            settings.deadline_ms,
            tab_id,
            trigger,
            structural_verdict,
        );
    }

    pub(crate) fn capture_exit_tab_observation(
        &self,
        tab_id: &str,
    ) -> Option<(DecisionQuestionsRequest, TabStateMetadata)> {
        let config = self.supporter_decision_config();
        let tokens = TokenSnapshot::read(
            &crate::api::mem_routes::decision::token_store(),
            config.active_id().unwrap_or(""),
        );
        if !supporter::supporter_connected(&config, &self.supporter_models, &tokens) {
            return None;
        }
        let adapter = supporter::configured_model(&config, &self.supporter_models)?;
        let mut prepared = prepare_observation(
            self,
            tab_id,
            ObservationTrigger::TabExit,
            "provider_process_exit",
        )
        .ok()?;
        prepared.1.envelope.as_mut()?.adapter = Some(adapter);
        Some(prepared)
    }

    pub(crate) fn observe_captured_tab_exit(
        &self,
        tab_id: &str,
        captured: Option<(DecisionQuestionsRequest, TabStateMetadata)>,
    ) {
        let Some(prepared) = captured else {
            return;
        };
        let config = crate::config::load_config().supporter;
        observer::enqueue_prepared(
            self,
            &config,
            config.active_id().unwrap_or(""),
            config.deadline_ms,
            tab_id,
            ObservationTrigger::TabExit,
            "provider_process_exit",
            prepared,
        );
    }

    pub(crate) fn shadow_tab_state(&self, tab_id: &str, structural: StartupNudge) {
        self.observe_tab_state(
            tab_id,
            ObservationTrigger::SpawnDeadline,
            structural.reason(),
        );
    }

    pub(crate) fn observe_worker_idle_without_report(&self, tab_id: &str) {
        let general = {
            self.mux
                .read()
                .get_tab(tab_id)
                .is_some_and(super::general_agent::is_general_agent)
        };
        if general {
            self.observe_tab_state(tab_id, ObservationTrigger::WorkerIdle, "general_agent_idle");
            return;
        }
        let snapshot = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if tab.role != Some(codeterm_types::agent::AgentRole::Worker)
                || tab.task_binding.is_none()
            {
                return;
            }
            (
                tab.parent_tab_id
                    .clone()
                    .or_else(|| tab.spawned_by_tab_id.clone()),
                tab.task_binding
                    .as_ref()
                    .and_then(|binding| binding.plan_id.clone()),
            )
        };
        let parent = snapshot
            .1
            .as_deref()
            .and_then(|plan_id| self.plan_store.get(plan_id))
            .and_then(|plan| plan.owner.tab_id)
            .or(snapshot.0);
        let last_turn_output = state::last_output_epoch_seconds(tab_id);
        if parent.as_deref().is_some_and(|parent| {
            self.report_store
                .lock()
                .has_report_from_after(parent, tab_id, last_turn_output)
        }) {
            return;
        }
        self.observe_tab_state(
            tab_id,
            ObservationTrigger::WorkerIdle,
            "worker_idle_without_report",
        );
    }
}

pub(crate) fn screen_tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(SCREEN_LINES)..].join("\n")
}

pub(crate) fn frame_hash(frame: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    frame.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "supporter_tab_state/tests.rs"]
mod tests;
