use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::supporter::{self, Action, ActionRecord, SupporterRuntime, TokenSnapshot, VerdictKind};
use codeterm_decision::{
    DecisionAnswer, DecisionModel, DecisionQuestionsRequest, DecisionResponse,
};
use codeterm_types::config::SupporterConfig;
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
use crate::service::supporter::registry::SupporterRegistry;

mod admission;
mod candidates;
pub(crate) mod coordinator;
pub(crate) use candidates::{allows_prompt as candidate_allows_prompt, CandidateVerdict};
mod delivery;
mod identity;
mod idle_readiness;
mod observer;
mod privacy;
pub(super) mod scheduler;
pub(crate) mod sentinel_request;

pub(crate) use identity::correlation::project_diagnostics as project_observation_diagnostics;
#[cfg(test)]
use observer::{choice_model, observation_answers, observe_guarded};
pub(crate) use observer::{prepare_observation, spawn_observer};
pub(crate) fn forget_observations(tab: &str) {
    admission::forget(tab);
}
pub(crate) use privacy::value as redact_decision_diagnostic;
pub(crate) use privacy::{screen as redact_screen, text as redact_text};
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

#[cfg(test)]
#[path = "supporter_tab_state/user_delivery_readiness_tests.rs"]
mod user_delivery_readiness_tests;

const CRITERIA_JSON: &str = include_str!("../../../../../evals/tab-state-jev/criteria.json");
const SPAWN_OK_LINE: &str = "spawn ok";

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
    #[cfg(test)]
    PromptCandidate,
    OutputCandidate,
    ParkedEdge,
    PromptEstablished,
    PromptConfirmation,
    SentinelRequest,
    WorkerIdle,
    BusyNoOutput,
    TabExit,
    UserDeliveryReadiness,
}

impl ObservationTrigger {
    fn event(self) -> ObservationEvent {
        match self {
            Self::SpawnReady => ObservationEvent::SpawnReady,
            Self::SpawnDeadline => ObservationEvent::SpawnDeadline,
            Self::StartupEscalation => ObservationEvent::StartupEscalation,
            #[cfg(test)]
            Self::PromptCandidate => ObservationEvent::PromptCandidate,
            Self::OutputCandidate
            | Self::ParkedEdge
            | Self::PromptEstablished
            | Self::PromptConfirmation
            | Self::SentinelRequest => ObservationEvent::PromptCandidate,
            Self::WorkerIdle => ObservationEvent::WorkerIdle,
            Self::BusyNoOutput => ObservationEvent::BusyNoOutput,
            Self::TabExit => ObservationEvent::TabExit,
            Self::UserDeliveryReadiness => ObservationEvent::PromptCandidate,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SpawnReady => "launch_ready",
            Self::SpawnDeadline => "startup_deadline",
            Self::StartupEscalation => "startup_escalation",
            #[cfg(test)]
            Self::PromptCandidate => "prompt_candidate",
            Self::OutputCandidate
            | Self::ParkedEdge
            | Self::PromptEstablished
            | Self::PromptConfirmation
            | Self::SentinelRequest => "prompt_candidate",
            Self::WorkerIdle => "worker_idle_without_report",
            Self::BusyNoOutput => "busy_without_output",
            Self::TabExit => "tab_exit",
            Self::UserDeliveryReadiness => "user_delivery_readiness",
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
            Self::SentinelRequest => Producer::SentinelRequest,
            _ => Producer::LifecycleEvent,
        }
    }

    fn is_prompt_candidate(self) -> bool {
        match self {
            #[cfg(test)]
            Self::PromptCandidate => true,
            Self::OutputCandidate
            | Self::ParkedEdge
            | Self::PromptEstablished
            | Self::PromptConfirmation
            | Self::SentinelRequest => true,
            _ => false,
        }
    }

    fn is_internal_candidate(self) -> bool {
        matches!(
            self,
            Self::PromptEstablished | Self::PromptConfirmation | Self::SentinelRequest
        )
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
    pub(crate) reported_prompt_visible: f64,
    pub(crate) input_area: Option<(String, f64)>,
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
            model.token_mode() != codeterm_decision::TokenMode::Required
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
        if envelope.pty_session_generation.is_some_and(|generation| {
            metadata
                .tab_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.pty_id.as_deref())
                .is_none_or(|pty_id| {
                    !self
                        .pty
                        .lock()
                        .session_generation_matches(pty_id, generation)
                })
        }) {
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
            && envelope.pty_session_generation.is_none_or(|generation| {
                metadata
                    .tab_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.pty_id.as_deref())
                    .is_some_and(|pty_id| {
                        self.pty
                            .lock()
                            .session_generation_matches(pty_id, generation)
                    })
            })
    }

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
        let token = supporter::read_token_snapshot(
            &self.supporter_runtime(),
            &crate::service::supporter::registry::token_store(),
            adapter,
        );
        settings.active_id() == Some(adapter)
            && supporter::supporter_connected(&settings, &self.supporter_models, &token)
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
        supporter::read_token_snapshot(
            &self.supporter_runtime(),
            &crate::service::supporter::registry::token_store(),
            adapter_id,
        )
        .usable
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
        let structural_classification = self
            .agent_registry
            .with_provider(provider_id, |provider| {
                provider.classify_tab_state(&crate::agent::tab_state::TabStateSnapshot {
                    tab_id: tab.id.clone(),
                    agent_type: provider_id.into(),
                    session_id: tab.agent_session_id.clone(),
                    cwd: tab.cwd.clone(),
                    screen_text: screen.into(),
                })
            })
            .flatten()
            .map(|classification| serde_json::json!({ "state": classification.state, "confidence": classification.confidence }));
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
                "classification": structural_classification,
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
            if trigger == ObservationTrigger::SentinelRequest {
                self.add_sentinel_safe_response_metadata(tab_id, screen, &mut metadata);
            } else {
                self.add_safe_response_metadata(tab_id, screen, &mut metadata);
            }
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
    pub(crate) fn user_delivery_pty_session_generation(&self, tab_id: &str) -> Result<u64, String> {
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone())
            .ok_or_else(|| "user delivery PTY identity unavailable".to_string())?;
        self.pty
            .lock()
            .get_session(&pty_id)
            .map(|session| session.generation)
            .ok_or_else(|| "user delivery PTY session generation unavailable".to_string())
    }

    pub(crate) fn supporter_decision_config(&self) -> SupporterConfig {
        #[cfg(test)]
        if let Some((config, _)) = TEST_SUPPORTER_OVERRIDES
            .lock()
            .get(&(Arc::as_ptr(&self.mux) as usize))
        {
            return config.clone();
        }
        self.configuration().supporter
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

    pub(crate) fn observe_user_delivery_readiness(
        &self,
        tab_id: &str,
        previous: Option<&UserDeliveryObservation>,
    ) -> Result<UserDeliveryObservation, String> {
        let expected_local = previous
            .and_then(|observation| observation.metadata.envelope.as_ref())
            .and_then(|envelope| envelope.local.as_ref());
        observer::observe_user_delivery_readiness(self, tab_id, expected_local)
    }

    pub(crate) fn user_delivery_observation_is_current(
        &self,
        observation: &UserDeliveryObservation,
    ) -> bool {
        observation.decided_by == identity::DecisionSource::Adapter
            && self.connection_is_current(&observation.adapter_id)
            && self
                .supporter_models()
                .get(&observation.adapter_id)
                .is_some_and(|current| Arc::ptr_eq(&current, &observation.adapter))
            && self.observation_is_current(&observation.tab_id, &observation.metadata)
            && observation
                .metadata
                .envelope
                .as_ref()
                .and_then(|envelope| envelope.pty_session_generation)
                == Some(observation.pty_session_generation)
            && self.user_delivery_pty_session_generation(&observation.tab_id)
                == Ok(observation.pty_session_generation)
    }

    pub(crate) fn capture_exit_tab_observation(
        &self,
        tab_id: &str,
    ) -> Option<(DecisionQuestionsRequest, TabStateMetadata)> {
        let config = self.supporter_decision_config();
        let tokens = supporter::read_token_snapshot(
            &supporter::runtime_with_notifications(self),
            &crate::service::supporter::registry::token_store(),
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
        let config = self.configuration().supporter;
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
        let (general, role, task_binding, parent_tab_id, plan_id) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if !idle_readiness::idle_observation_ready(
                tab.launch_session_authenticated(),
                tab.agent_booting,
                tab.boot_stage,
            ) {
                return;
            }
            (
                super::general_agent::is_general_agent(tab),
                tab.role,
                tab.task_binding.is_some(),
                tab.parent_tab_id
                    .clone()
                    .or_else(|| tab.spawned_by_tab_id.clone()),
                tab.task_binding
                    .as_ref()
                    .and_then(|binding| binding.plan_id.clone()),
            )
        };
        if general {
            self.observe_tab_state(tab_id, ObservationTrigger::WorkerIdle, "general_agent_idle");
            return;
        }
        if role != Some(codeterm_types::agent::AgentRole::Worker) || !task_binding {
            return;
        }
        let parent = plan_id
            .as_deref()
            .and_then(|plan_id| self.plan_store.get(plan_id))
            .and_then(|plan| plan.owner.tab_id)
            .or(parent_tab_id);
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

#[derive(Clone)]
pub(crate) struct UserDeliveryObservation {
    pub(crate) tab_id: String,
    pub(crate) adapter_id: String,
    adapter: Arc<dyn DecisionModel>,
    pub(crate) observation_id: u64,
    pub(super) pty_session_generation: u64,
    pub(crate) decided_by: identity::DecisionSource,
    pub(crate) metadata: TabStateMetadata,
    pub(crate) verdict: ShadowVerdict,
}

#[derive(Clone)]
pub(crate) struct UserDeliveryReadiness {
    observations: [UserDeliveryObservation; 2],
}

impl UserDeliveryReadiness {
    pub fn new(
        first: UserDeliveryObservation,
        second: UserDeliveryObservation,
    ) -> Result<Self, &'static str> {
        if first.observation_id == second.observation_id {
            return Err("observations_not_distinct");
        }
        let first_local = first
            .metadata
            .envelope
            .as_ref()
            .and_then(|envelope| envelope.local.as_ref());
        let second_local = second
            .metadata
            .envelope
            .as_ref()
            .and_then(|envelope| envelope.local.as_ref());
        if first.tab_id != second.tab_id
            || first.adapter_id != second.adapter_id
            || !Arc::ptr_eq(&first.adapter, &second.adapter)
            || !user_delivery_pair_has_same_generation(
                first.observation_id,
                second.observation_id,
                first.pty_session_generation,
                second.pty_session_generation,
                first_local,
                second_local,
            )
        {
            return Err("observation_generation_changed");
        }
        if !first.qualifies() || !second.qualifies() {
            return Err(first
                .pending_reason()
                .or_else(|| second.pending_reason())
                .unwrap_or("supporter_verdict_not_ready_idle"));
        }
        Ok(Self {
            observations: [first, second],
        })
    }

    pub(crate) fn current_observation(&self) -> &UserDeliveryObservation {
        &self.observations[1]
    }
}

fn user_delivery_pair_has_same_generation(
    first_id: u64,
    second_id: u64,
    first_pty_session_generation: u64,
    second_pty_session_generation: u64,
    first: Option<&identity::LocalIdentity>,
    second: Option<&identity::LocalIdentity>,
) -> bool {
    first_id != second_id
        && first_pty_session_generation == second_pty_session_generation
        && first
            .zip(second)
            .is_some_and(|(first, second)| first == second)
}

impl UserDeliveryObservation {
    pub(crate) fn qualifies(&self) -> bool {
        user_delivery_verdict_qualifies(
            self.decided_by,
            &self.verdict.label,
            self.verdict.confidence,
            self.verdict.reported_prompt_visible,
            self.verdict
                .input_area
                .as_ref()
                .map(|(area, confidence)| (area.as_str(), *confidence)),
        )
    }

    pub(crate) fn pending_reason(&self) -> Option<&'static str> {
        let source_reason = match self.decided_by {
            identity::DecisionSource::Adapter => None,
            identity::DecisionSource::NoAdapter => Some("supporter_no_adapter"),
            identity::DecisionSource::Timeout => Some("supporter_timeout"),
            identity::DecisionSource::Error(_) => Some("supporter_error"),
            identity::DecisionSource::Budget => Some("supporter_budget"),
            identity::DecisionSource::Other => Some("supporter_other"),
            identity::DecisionSource::LowConfidence => Some("supporter_low_confidence"),
        };
        source_reason
            .or_else(|| {
                (self.verdict.label != READY_IDLE).then_some("supporter_verdict_not_ready_idle")
            })
            .or_else(|| {
                (self.verdict.confidence < 0.90).then_some("supporter_confidence_below_threshold")
            })
            .or_else(|| {
                (self.verdict.reported_prompt_visible > 0.10).then_some("supporter_prompt_visible")
            })
            .or_else(|| {
                self.verdict.input_area.as_ref().map_or(
                    Some("supporter_input_area_unavailable"),
                    |(area, confidence)| {
                        if area != "composer" {
                            Some("supporter_input_area_not_composer")
                        } else if *confidence < 0.90 {
                            Some("supporter_input_area_low_confidence")
                        } else {
                            None
                        }
                    },
                )
            })
    }
}

fn user_delivery_verdict_qualifies(
    decided_by: identity::DecisionSource,
    label: &str,
    confidence: f64,
    prompt_visible: f64,
    input_area: Option<(&str, f64)>,
) -> bool {
    decided_by == identity::DecisionSource::Adapter
        && label == READY_IDLE
        && confidence >= 0.90
        && prompt_visible <= 0.10
        && input_area.is_some_and(|(area, confidence)| area == "composer" && confidence >= 0.90)
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
