use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::startup_nudge::StartupNudge;
use super::tab_transport::TabTransport;
use super::ServiceContext;
use crate::api::mem_routes::decision::SupporterRegistry;
use crate::mem::decision::{DecisionAnswer, DecisionModel, DecisionQuestion, DecisionRequest};
use codeterm_types::config::SupporterTabStateConfig;

mod delivery;
mod state;

pub(crate) use state::{
    apply_observed_verdict, claim_observation, confidence_band, note_output, permit_is_current,
    ConfidenceBand, ObservationEvent, PermitError, SignalAction, CALL_CAP_PER_EPISODE,
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
pub(crate) const STILL_STARTING: &str = "still_starting";

const CRITERIA_JSON: &str = include_str!("../../../evals/tab-state-jev/criteria.json");
const SPAWN_OK_LINE: &str = "spawn ok";
const WEAK_HINT_PREFIX: &str = "Tab-state hint";

fn criteria() -> &'static BTreeMap<String, String> {
    static CRITERIA: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CRITERIA.get_or_init(|| {
        serde_json::from_str(CRITERIA_JSON).expect("embedded tab-state criteria are valid")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationTrigger {
    SpawnReady,
    SpawnDeadline,
    WorkerIdle,
    BusyNoOutput,
    TabExit,
}

impl ObservationTrigger {
    fn event(self) -> ObservationEvent {
        match self {
            Self::SpawnReady => ObservationEvent::SpawnReady,
            Self::SpawnDeadline => ObservationEvent::SpawnDeadline,
            Self::WorkerIdle => ObservationEvent::WorkerIdle,
            Self::BusyNoOutput => ObservationEvent::BusyNoOutput,
            Self::TabExit => ObservationEvent::TabExit,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SpawnReady => "launch_ready",
            Self::SpawnDeadline => "startup_deadline",
            Self::WorkerIdle => "worker_idle_without_report",
            Self::BusyNoOutput => "busy_without_output",
            Self::TabExit => "tab_exit",
        }
    }

    fn is_spawn(self) -> bool {
        matches!(self, Self::SpawnReady | Self::SpawnDeadline)
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
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShadowVerdict {
    pub(crate) label: String,
    pub(crate) confidence: f64,
    pub(crate) action: SignalAction,
}

pub(crate) trait ShadowHost: Clone + Send + 'static {
    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String>;
    fn supporter_models(&self) -> Arc<SupporterRegistry>;
    fn token_available(&self, adapter_id: &str) -> bool;
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
    fn record(&self, line: &str);
}

impl ShadowHost for ServiceContext {
    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        self.tab_transport(tab_id)
    }

    fn supporter_models(&self) -> Arc<SupporterRegistry> {
        self.supporter_models.clone()
    }

    fn token_available(&self, adapter_id: &str) -> bool {
        let store = crate::mem::decision::token_store::TokenStore::new(
            crate::api::mem_routes::decision::supporter_token_dir(),
        );
        match store.status(adapter_id) {
            Ok(status) => status.present && status.mode_ok != Some(false),
            Err(error) => {
                log::debug!("[tab-state] token status unavailable: {error}");
                false
            }
        }
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
        let session_key = tab
            .session_binding_nonce
            .clone()
            .or_else(|| tab.pty_id.clone())
            .or_else(|| tab.agent_session_id.clone())
            .unwrap_or_else(|| format!("{}:{:?}", tab.id, tab.agent_set_at));
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        screen.hash(&mut hasher);
        let changed = state::screen_changed(&tab.id, &session_key, hasher.finish());
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
        Ok(TabStateMetadata {
            context,
            session_key,
            title: tab.title,
            spawning_tab_id: tab.spawned_by_tab_id,
            parent_tab_id: tab.parent_tab_id,
            plan_id: task_binding.and_then(|binding| binding.plan_id.clone()),
            task_id: task_binding.and_then(|binding| binding.task_id.clone()),
            attempt_id: task_binding.and_then(|binding| binding.attempt_id.clone()),
            prompt_options,
        })
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

    fn record(&self, line: &str) {
        log::info!("{line}");
    }
}

impl ServiceContext {
    pub(crate) fn observe_launch_settlement_tab_state(
        &self,
        tab_id: &str,
        settlement: &crate::service::launch_readiness::LaunchSettlement,
    ) {
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
        let settings = crate::config::load_config().supporter;
        drop(spawn_observer(
            self,
            &settings.tab_state,
            &settings.adapter,
            settings.deadline_ms,
            tab_id,
            trigger,
            structural_verdict,
        ));
    }

    pub(crate) fn shadow_tab_state(&self, tab_id: &str, structural: StartupNudge) {
        self.observe_tab_state(
            tab_id,
            ObservationTrigger::SpawnDeadline,
            structural.reason(),
        );
    }

    pub(crate) fn observe_worker_idle_without_report(&self, tab_id: &str) {
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
                tab.parent_tab_id.clone().or_else(|| tab.spawned_by_tab_id.clone()),
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

pub(crate) fn spawn_observer<H: ShadowHost>(
    host: &H,
    config: &SupporterTabStateConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Option<JoinHandle<Option<ShadowVerdict>>> {
    if !config.enabled {
        return None;
    }
    let host = host.clone();
    let tab_id = tab_id.to_owned();
    let structural_verdict = structural_verdict.to_owned();
    let options = config.clone();
    let adapter_id = adapter_id.to_owned();
    std::thread::Builder::new()
        .name("tab-state-observer".into())
        .spawn(move || {
            observe_guarded(
                &host,
                &options,
                &adapter_id,
                configured_deadline_ms,
                &tab_id,
                trigger,
                &structural_verdict,
            )
        })
        .map_err(|error| {
            log::debug!("[tab-state] observer thread failed tab={tab_id}: {error}");
        })
        .ok()
}

fn observe_guarded<H: ShadowHost>(
    host: &H,
    config: &SupporterTabStateConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Option<ShadowVerdict> {
    if !host.token_available(adapter_id) {
        log::debug!("[tab-state] no usable token; tab={tab_id} observation skipped");
        return None;
    }
    let models = host.supporter_models();
    let Some(model) = choice_model(&models, adapter_id) else {
        log::debug!("[tab-state] no choice adapter; tab={tab_id} observation skipped");
        return None;
    };
    let deadline = Instant::now()
        + Duration::from_millis(configured_deadline_ms.max(1)).min(DEADLINE);
    let (request, metadata) = match prepare_observation(
        host,
        adapter_id,
        tab_id,
        trigger,
        structural_verdict,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            host.record(&format!("[tab-state] tab={tab_id} no verdict: {error}"));
            return None;
        }
    };
    let permit = match claim_observation(tab_id, &metadata.session_key, trigger.event()) {
        Ok(permit) => permit,
        Err(reason) => {
            log::debug!("[tab-state] observation skipped tab={tab_id}: {reason:?}");
            return None;
        }
    };
    observe(
        host,
        &model,
        config,
        adapter_id,
        tab_id,
        trigger,
        deadline,
        request,
        metadata,
        permit,
    )
}

fn observe<H: ShadowHost>(
    host: &H,
    model: &Arc<dyn DecisionModel>,
    config: &SupporterTabStateConfig,
    adapter_id: &str,
    tab_id: &str,
    trigger: ObservationTrigger,
    deadline: Instant,
    request: DecisionRequest,
    metadata: TabStateMetadata,
    permit: state::ObservationPermit,
) -> Option<ShadowVerdict> {
    if !host.token_available(adapter_id) {
        log::debug!("[tab-state] token expired before choice; observation skipped");
        return None;
    }
    match model
        .decide(&request, deadline)
        .map_err(|error| error.to_string())
        .and_then(chosen_label)
    {
        Ok((label, confidence)) => {
            let mut verdict = ShadowVerdict {
                label,
                confidence,
                action: SignalAction::None,
            };
            let spawn_healthy = trigger.is_spawn()
                && confidence_band(verdict.confidence) == ConfidenceBand::Strong
                && matches!(verdict.label.as_str(), READY_IDLE | WORKING_NORMALLY);
            if verdict.label == STILL_STARTING && !trigger.after_startup_deadline() {
                host.record(&format!(
                    "[tab-state] suppressed early still_starting tab={tab_id}"
                ));
                return Some(verdict);
            }
            verdict.action = if spawn_healthy {
                if !permit_is_current(tab_id, &metadata.session_key, &permit) {
                    host.record(&format!(
                        "[tab-state] stale observation discarded tab={tab_id}"
                    ));
                    return None;
                }
                SignalAction::None
            } else {
                let Some(action) = apply_observed_verdict(
                    tab_id,
                    &metadata.session_key,
                    &permit,
                    &verdict.label,
                    verdict.confidence,
                    config.strong_actions,
                    config.weak_hints,
                ) else {
                    host.record(&format!(
                        "[tab-state] stale observation discarded tab={tab_id}"
                    ));
                    return None;
                };
                action
            };
            if spawn_healthy {
                host.deliver(
                    tab_id,
                    &metadata,
                    permit.episode,
                    trigger,
                    &verdict.label,
                    verdict.confidence,
                    SignalAction::SpawnOk,
                );
            }
            if verdict.action != SignalAction::None && !spawn_healthy {
                host.deliver(
                    tab_id,
                    &metadata,
                    permit.episode,
                    trigger,
                    &verdict.label,
                    verdict.confidence,
                    verdict.action,
                );
            }
            host.record(&format!(
                "[tab-state] tab={tab_id} event={} label={} confidence={:.3} episode={} calls_capped={}",
                trigger.label(),
                verdict.label,
                verdict.confidence,
                permit.episode,
                CALL_CAP_PER_EPISODE,
            ));
            Some(verdict)
        }
        Err(error) => {
            host.record(&format!("[tab-state] tab={tab_id} no verdict: {error}"));
            None
        }
    }
}

fn prepare_observation<H: ShadowHost>(
    host: &H,
    adapter_id: &str,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Result<(DecisionRequest, TabStateMetadata), String> {
    let screen = {
        let transport = host.transport(tab_id)?;
        transport.read_screen(false)?
    };
    let text = screen
        .get("text")
        .and_then(Value::as_str)
        .ok_or("screen read returned no text")?;
    let metadata = host.metadata(tab_id, trigger, structural_verdict, text)?;
    if !host.token_available(adapter_id) {
        return Err("decision token is no longer available".into());
    }
    let request = DecisionRequest {
        state: serde_json::json!({
            "screen": screen_tail(text),
            "context": metadata.context,
        }),
        instructions: INSTRUCTIONS.to_owned(),
        question: DecisionQuestion::Choice {
            options: criteria().clone(),
        },
    };
    Ok((request, metadata))
}

pub(crate) fn choice_model(
    models: &SupporterRegistry,
    adapter_id: &str,
) -> Option<Arc<dyn DecisionModel>> {
    models
        .get(adapter_id)
        .filter(|model| model.primitives().choice)
}

pub(crate) fn screen_tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(SCREEN_LINES)..].join("\n")
}

fn chosen_label(answer: DecisionAnswer) -> Result<(String, f64), String> {
    let DecisionAnswer::Choice {
        choice,
        probabilities,
        confidence,
    } = answer
    else {
        return Err("decision model answered a non-choice question".into());
    };
    if !criteria().contains_key(&choice) {
        return Err("decision model chose an unknown label".into());
    }
    let confidence = confidence
        .or_else(|| probabilities.get(&choice).copied())
        .filter(|confidence| confidence.is_finite() && (0.0..=1.0).contains(confidence))
        .ok_or("decision model gave no usable confidence")?;
    Ok((choice, confidence))
}

#[cfg(test)]
#[path = "supporter_tab_state/tests.rs"]
mod tests;
