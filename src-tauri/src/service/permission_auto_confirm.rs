//! Opt-in per-tab auto-confirm of provider permission prompts. Only a prompt
//! backed by a hook-reported permission request and recognized by the
//! provider's own capability is answered, once, with its affirmative option.

use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use codeterm_types::pty::{record_auto_confirmation, AutoConfirmedPrompt};
use sha2::{Digest, Sha256};

use super::ServiceContext;
use crate::agent::permission_request::PermissionRequestSignal;
use crate::agent::prompt::{PermissionRequestInstance, PromptDecision};
use crate::multiplexer::types::{PendingPrompt, Tab};
use crate::session::lifecycle::{LifecycleTransition, PermissionAutoConfirmDecision};

/// A permission request older than this no longer vouches for a prompt.
pub(crate) const REQUEST_MAX_AGE: Duration = Duration::from_secs(120);
pub(crate) const RATE_WINDOW: Duration = Duration::from_secs(60);
pub(crate) const RATE_LIMIT: usize = 10;
/// After an answer, the same prompt still on screen is the answer in flight.
pub(crate) const SETTLE_WINDOW: Duration = Duration::from_secs(10);
const ANSWERED_MEMORY: usize = 64;

static LEDGER: LazyLock<parking_lot::Mutex<Ledger>> =
    LazyLock::new(|| parking_lot::Mutex::new(Ledger::default()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    Disabled,
    NoPermissionRequest,
    StaleRequest,
    SessionUnbound,
    NoLifecycle,
    NotRecognized,
    RateLimited,
    AlreadyAnswered,
    SessionMoved,
    PtyMoved,
    PromptChanged,
}

impl Refusal {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NoPermissionRequest => "no_permission_request",
            Self::StaleRequest => "stale_permission_request",
            Self::SessionUnbound => "session_unbound",
            Self::NoLifecycle => "no_lifecycle_operation",
            Self::NotRecognized => "not_recognized",
            Self::RateLimited => "rate_limited",
            Self::AlreadyAnswered => "already_answered",
            Self::SessionMoved => "session_moved",
            Self::PtyMoved => "pty_moved",
            Self::PromptChanged => "prompt_changed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    NotApplicable,
    Confirmed,
    /// The prompt on screen is one this tab already answered; keep it hidden.
    Settling,
    Refused(Refusal),
}

fn manual_update_prompt_verdict(
    classification: Option<&codeterm_plugin::tab_state::TabStateClassification>,
) -> Option<Verdict> {
    crate::agent::state_reactor::is_manual_update_prompt(classification)
        .then_some(Verdict::Refused(Refusal::NotRecognized))
}

impl Verdict {
    pub(crate) fn owns_prompt(self) -> bool {
        matches!(self, Self::Confirmed | Self::Settling)
    }
}

/// What the decision reads from the tab, captured under one mux read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabFacts {
    pub enabled: bool,
    pub agent_type: Option<String>,
    pub pty_id: Option<String>,
    pub session_id: Option<String>,
    pub session_authenticated: bool,
    pub has_lifecycle: bool,
}

impl TabFacts {
    pub(crate) fn of(tab: &Tab, has_lifecycle: bool) -> Self {
        Self {
            enabled: tab.permission_auto_confirm,
            agent_type: tab.agent_type.clone(),
            pty_id: tab.pty_id.clone(),
            session_id: tab.agent_session_id.clone(),
            session_authenticated: tab.launch_session_authenticated(),
            has_lifecycle,
        }
    }
}

/// The exact tab, session, PTY and request instance an answer is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptBinding {
    pub tab_id: String,
    pub session_id: String,
    pub pty_id: String,
    pub request_id: u64,
    pub screen_fingerprint: String,
    pub prompt_fingerprint: String,
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Identity of the prompt as rendered: its question and option labels.
pub(crate) fn screen_fingerprint(prompt: &PendingPrompt) -> String {
    let mut parts = vec![prompt.question.as_str()];
    parts.extend(prompt.options.iter().map(|o| o.label.as_str()));
    digest(&parts)
}

pub(crate) fn prompt_fingerprint(
    tab_id: &str,
    session_id: &str,
    pty_id: &str,
    request_id: u64,
    signal: &PermissionRequestSignal,
    prompt: &PendingPrompt,
) -> String {
    let request = request_id.to_string();
    let input = signal.tool_input.to_string();
    let rendered = screen_fingerprint(prompt);
    digest(&[
        tab_id,
        session_id,
        pty_id,
        &request,
        &signal.tool_name,
        &input,
        &rendered,
    ])
}

/// Preconditions that must hold before the provider is even asked.
pub(crate) fn precheck(
    facts: &TabFacts,
    request: PermissionRequestInstance,
    now: Instant,
) -> Result<(String, String), Refusal> {
    if !facts.enabled {
        return Err(Refusal::Disabled);
    }
    let pty_id = facts.pty_id.clone().ok_or(Refusal::PtyMoved)?;
    if now.saturating_duration_since(request.received_at) > REQUEST_MAX_AGE {
        return Err(Refusal::StaleRequest);
    }
    let session_id = facts
        .session_id
        .clone()
        .filter(|_| facts.session_authenticated)
        .ok_or(Refusal::SessionUnbound)?;
    if !facts.has_lifecycle {
        return Err(Refusal::NoLifecycle);
    }
    Ok((session_id, pty_id))
}

/// Re-checked immediately before the keys are written; turning the setting
/// off between decision and write refuses here.
pub(crate) fn verify_binding(
    bound: &PromptBinding,
    facts: Option<&TabFacts>,
    live_screen_fingerprint: Option<&str>,
) -> Result<(), Refusal> {
    let facts = facts.ok_or(Refusal::PtyMoved)?;
    if !facts.enabled {
        return Err(Refusal::Disabled);
    }
    if !facts.session_authenticated || facts.session_id.as_deref() != Some(&bound.session_id) {
        return Err(Refusal::SessionMoved);
    }
    if facts.pty_id.as_deref() != Some(&bound.pty_id) {
        return Err(Refusal::PtyMoved);
    }
    if live_screen_fingerprint != Some(bound.screen_fingerprint.as_str()) {
        return Err(Refusal::PromptChanged);
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct Ledger {
    tabs: HashMap<String, TabLedger>,
}

#[derive(Debug, Default)]
struct TabLedger {
    answered: VecDeque<u64>,
    answer_times: VecDeque<Instant>,
    settling: Option<(String, Instant)>,
    last_refusal: Option<(u64, Refusal)>,
}

impl Ledger {
    /// Reserve the single answer for a request instance, rate-limited per tab.
    pub(crate) fn claim(
        &mut self,
        tab_id: &str,
        request_id: u64,
        screen_fingerprint: &str,
        now: Instant,
    ) -> Result<(), Refusal> {
        let tab = self.tabs.entry(tab_id.to_string()).or_default();
        if tab.answered.contains(&request_id) {
            return Err(Refusal::AlreadyAnswered);
        }
        while tab
            .answer_times
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= RATE_WINDOW)
        {
            tab.answer_times.pop_front();
        }
        if tab.answer_times.len() >= RATE_LIMIT {
            return Err(Refusal::RateLimited);
        }
        tab.answered.push_back(request_id);
        if tab.answered.len() > ANSWERED_MEMORY {
            tab.answered.pop_front();
        }
        tab.answer_times.push_back(now);
        tab.settling = Some((screen_fingerprint.to_string(), now));
        Ok(())
    }

    pub(crate) fn settling(&self, tab_id: &str, screen_fingerprint: &str, now: Instant) -> bool {
        self.tabs
            .get(tab_id)
            .and_then(|tab| tab.settling.as_ref())
            .is_some_and(|(fingerprint, at)| {
                fingerprint == screen_fingerprint
                    && now.saturating_duration_since(*at) < SETTLE_WINDOW
            })
    }

    pub(crate) fn end_settling(&mut self, tab_id: &str) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.settling = None;
        }
    }

    /// True the first time a request is refused for this reason.
    pub(crate) fn note_refusal(&mut self, tab_id: &str, request_id: u64, refusal: Refusal) -> bool {
        let tab = self.tabs.entry(tab_id.to_string()).or_default();
        let fresh = tab.last_refusal != Some((request_id, refusal));
        tab.last_refusal = Some((request_id, refusal));
        fresh
    }

    pub(crate) fn forget(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn audit_transition(
    decision: PermissionAutoConfirmDecision,
    tool_name: Option<&str>,
    prompt: Option<&PendingPrompt>,
) -> LifecycleTransition {
    LifecycleTransition::PermissionAutoConfirm {
        decision,
        tool_name: tool_name.map(str::to_string),
        option_label: None,
        rule: None,
        question: prompt.map(|p| p.question.clone()),
        prompt_fingerprint: None,
        session_id: None,
        reason: None,
    }
}

impl ServiceContext {
    fn permission_auto_confirm_facts(&self, tab_id: &str) -> Option<TabFacts> {
        let mux = self.mux.read();
        let has_lifecycle = mux.lifecycle_key(tab_id).is_some();
        mux.get_tab(tab_id)
            .map(|tab| TabFacts::of(tab, has_lifecycle))
    }

    pub(crate) fn set_permission_auto_confirm(
        &self,
        tab_id: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        let changed = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            let changed = tab.permission_auto_confirm != enabled;
            tab.permission_auto_confirm = enabled;
            changed
        };
        if !changed {
            return Ok(enabled);
        }
        log::info!("[permission-auto-confirm] Tab {tab_id} enabled={enabled}");
        let decision = if enabled {
            PermissionAutoConfirmDecision::Enabled
        } else {
            PermissionAutoConfirmDecision::Disabled
        };
        self.record_lifecycle(tab_id, audit_transition(decision, None, None));
        if let Err(error) = super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        ) {
            log::warn!("[permission-auto-confirm] persist after toggle failed: {error}");
        }
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "permission-auto-confirm-changed",
            serde_json::json!({ "tab_id": tab_id, "enabled": enabled }),
        );
        self.emit_state_changed();
        Ok(enabled)
    }

    pub(crate) fn permission_auto_confirm_state(
        &self,
        tab_id: &str,
    ) -> Option<(bool, Vec<AutoConfirmedPrompt>)> {
        self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.permission_auto_confirm,
                tab.permission_auto_confirmations.clone(),
            )
        })
    }

    pub(crate) fn forget_permission_auto_confirm(&self, tab_id: &str) {
        LEDGER.lock().forget(tab_id);
    }

    /// Hook ingestion: a decision that changed nothing may still complete the
    /// request/screen pair an earlier report left waiting.
    pub(crate) fn apply_hook_prompt_decision(&self, tab_id: &str, decision: &PromptDecision) {
        match decision {
            PromptDecision::None => {
                self.try_permission_auto_confirm(tab_id, None);
            }
            _ => self.apply_prompt_decision(decision),
        }
    }

    pub(crate) fn try_permission_auto_confirm(
        &self,
        tab_id: &str,
        screen_text: Option<&str>,
    ) -> Verdict {
        let Some(facts) = self.permission_auto_confirm_facts(tab_id) else {
            return Verdict::NotApplicable;
        };
        let (true, Some(agent_type), Some(pty_id)) = (
            facts.enabled,
            facts.agent_type.clone(),
            facts.pty_id.clone(),
        ) else {
            return Verdict::NotApplicable;
        };
        let screen = match screen_text {
            Some(text) => text.to_string(),
            None => self
                .screen_state
                .lock()
                .get_screen_text(&pty_id)
                .unwrap_or_default(),
        };
        let snapshot = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id);
            codeterm_plugin::tab_state::TabStateSnapshot {
                tab_id: tab_id.to_string(),
                agent_type: agent_type.clone(),
                session_id: facts.session_id.clone(),
                cwd: tab.and_then(|tab| tab.cwd.clone()),
                screen_text: screen.clone(),
            }
        };
        let state = self
            .agent_registry
            .with_provider(&agent_type, |provider| {
                provider.classify_tab_state(&snapshot)
            })
            .flatten();
        if let Some(verdict) = manual_update_prompt_verdict(state.as_ref()) {
            return verdict;
        }
        let Some(prompt) = crate::agent::prompt::parse_prompt_safely(|| {
            self.agent_registry.parse_prompt(&agent_type, &screen)
        }) else {
            return Verdict::Refused(Refusal::NotRecognized);
        };
        let screen_fp = screen_fingerprint(&prompt);
        let now = Instant::now();
        let request = self
            .prompt_detector
            .lock()
            .pending_permission_request(tab_id);
        let Some((instance, signal)) = request else {
            let settling = LEDGER.lock().settling(tab_id, &screen_fp, now);
            if settling {
                self.prompt_detector.lock().mark_responded(tab_id);
                self.retract_surfaced_prompt(tab_id);
                return Verdict::Settling;
            }
            return Verdict::Refused(Refusal::NoPermissionRequest);
        };
        let (session_id, bound_pty) = match precheck(&facts, instance, now) {
            Ok(ids) => ids,
            Err(refusal) => return self.refuse(tab_id, instance.id, &signal, &prompt, refusal),
        };
        let response = self
            .agent_registry
            .with_provider(&agent_type, |provider| {
                provider.permission_prompt_response(&signal.tool_name, &prompt, &screen)
            })
            .flatten();
        let Some((response, option_label)) = response.and_then(|response| {
            let label = prompt.options.get(response.option_index)?.label.clone();
            Some((response, label))
        }) else {
            return self.refuse(
                tab_id,
                instance.id,
                &signal,
                &prompt,
                Refusal::NotRecognized,
            );
        };
        let claimed = LEDGER.lock().claim(tab_id, instance.id, &screen_fp, now);
        if let Err(refusal) = claimed {
            return self.refuse(tab_id, instance.id, &signal, &prompt, refusal);
        }
        let taken = self
            .prompt_detector
            .lock()
            .take_permission_request(tab_id, instance.id);
        if taken.is_none() {
            LEDGER.lock().end_settling(tab_id);
            return self.refuse(
                tab_id,
                instance.id,
                &signal,
                &prompt,
                Refusal::AlreadyAnswered,
            );
        }
        self.prompt_detector.lock().mark_responded(tab_id);
        self.retract_surfaced_prompt(tab_id);

        let binding = PromptBinding {
            prompt_fingerprint: prompt_fingerprint(
                tab_id,
                &session_id,
                &bound_pty,
                instance.id,
                &signal,
                &prompt,
            ),
            tab_id: tab_id.to_string(),
            session_id,
            pty_id: bound_pty.clone(),
            request_id: instance.id,
            screen_fingerprint: screen_fp,
        };
        let audit = AutoConfirmedPrompt {
            tool_name: signal.tool_name.clone(),
            option_label,
            rule: response.rule.clone(),
            question: prompt.question.clone(),
            options: prompt.options.iter().map(|o| o.label.clone()).collect(),
            prompt_fingerprint: binding.prompt_fingerprint.clone(),
            session_id: Some(binding.session_id.clone()),
            confirmed_at_ms: 0,
        };
        let svc = self.clone();
        super::output::write_bound_prompt_steps(
            self.pty.clone(),
            bound_pty,
            response.steps,
            move || svc.commit_bound_answer(&binding, &agent_type, prompt, audit),
        );
        Verdict::Confirmed
    }

    fn commit_bound_answer(
        &self,
        binding: &PromptBinding,
        agent_type: &str,
        prompt: PendingPrompt,
        mut audit: AutoConfirmedPrompt,
    ) -> bool {
        let facts = self.permission_auto_confirm_facts(&binding.tab_id);
        let live_fingerprint = facts
            .as_ref()
            .and_then(|facts| facts.pty_id.clone())
            .and_then(|pty_id| self.screen_state.lock().get_screen_text(&pty_id))
            .and_then(|screen| {
                crate::agent::prompt::parse_prompt_safely(|| {
                    self.agent_registry.parse_prompt(agent_type, &screen)
                })
            })
            .map(|live| screen_fingerprint(&live));
        if let Err(refusal) = verify_binding(binding, facts.as_ref(), live_fingerprint.as_deref()) {
            LEDGER.lock().end_settling(&binding.tab_id);
            log::warn!(
                "[permission-auto-confirm] refused {} for Tab {} at write: {}",
                audit.tool_name,
                binding.tab_id,
                refusal.label()
            );
            let mut transition = audit_transition(
                PermissionAutoConfirmDecision::Refused,
                Some(&audit.tool_name),
                Some(&prompt),
            );
            if let LifecycleTransition::PermissionAutoConfirm {
                prompt_fingerprint,
                session_id,
                reason,
                ..
            } = &mut transition
            {
                *prompt_fingerprint = Some(binding.prompt_fingerprint.clone());
                *session_id = Some(binding.session_id.clone());
                *reason = Some(refusal.label().to_string());
            }
            self.record_lifecycle(&binding.tab_id, transition);
            if refusal != Refusal::PromptChanged {
                self.resurface_prompt(&binding.tab_id, prompt);
            }
            return false;
        }

        audit.confirmed_at_ms = epoch_ms();
        log::info!(
            "[permission-auto-confirm] Tab {} auto-confirmed {} via {:?} (rule {}, prompt {})",
            binding.tab_id,
            audit.tool_name,
            audit.option_label,
            audit.rule,
            binding.prompt_fingerprint
        );
        self.record_lifecycle(
            &binding.tab_id,
            LifecycleTransition::PermissionAutoConfirm {
                decision: PermissionAutoConfirmDecision::Confirmed,
                tool_name: Some(audit.tool_name.clone()),
                option_label: Some(audit.option_label.clone()),
                rule: Some(audit.rule.clone()),
                question: Some(audit.question.clone()),
                prompt_fingerprint: Some(binding.prompt_fingerprint.clone()),
                session_id: Some(binding.session_id.clone()),
                reason: None,
            },
        );
        if let Some(tab) = self.mux.write().get_tab_mut(&binding.tab_id) {
            record_auto_confirmation(&mut tab.permission_auto_confirmations, audit.clone());
        }
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "permission-auto-confirmed",
            serde_json::json!({ "tab_id": binding.tab_id, "confirmation": audit }),
        );
        self.emit_state_changed();
        true
    }

    fn refuse(
        &self,
        tab_id: &str,
        request_id: u64,
        signal: &PermissionRequestSignal,
        prompt: &PendingPrompt,
        refusal: Refusal,
    ) -> Verdict {
        let first_report = LEDGER.lock().note_refusal(tab_id, request_id, refusal);
        if first_report {
            log::info!(
                "[permission-auto-confirm] left {} prompt open for Tab {}: {}",
                signal.tool_name,
                tab_id,
                refusal.label()
            );
            let mut transition = audit_transition(
                PermissionAutoConfirmDecision::Refused,
                Some(&signal.tool_name),
                Some(prompt),
            );
            if let LifecycleTransition::PermissionAutoConfirm { reason, .. } = &mut transition {
                *reason = Some(refusal.label().to_string());
            }
            self.record_lifecycle(tab_id, transition);
        }
        Verdict::Refused(refusal)
    }

    pub(crate) fn retract_surfaced_prompt(&self, tab_id: &str) {
        let (retracted, identity) = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id).map_or((false, None), |tab| {
                let retracted = tab.pending_prompt.is_some();
                let identity =
                    crate::service::prompt_resolution::clear_tab_prompt(tab_id, Some(tab));
                (retracted, identity)
            })
        };
        if let Some(identity) = identity {
            self.resolve_prompt_notifications(tab_id, &identity);
        }
        if retracted {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "prompt-state-changed",
                serde_json::json!({ "tab_id": tab_id, "prompt": null }),
            );
            self.emit_state_changed();
        }
    }

    fn resurface_prompt(&self, tab_id: &str, prompt: PendingPrompt) {
        let surfaced = match self.mux.write().get_tab_mut(tab_id) {
            Some(tab) => {
                tab.pending_prompt = Some(prompt.clone());
                tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
                true
            }
            None => false,
        };
        if surfaced {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "prompt-state-changed",
                serde_json::json!({ "tab_id": tab_id, "prompt": prompt }),
            );
            self.emit_state_changed();
        }
    }
}

#[cfg(test)]
mod tests;
