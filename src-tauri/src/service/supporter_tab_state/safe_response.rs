use super::*;
use crate::agent::prompt::parse_prompt_safely;
use codeterm_plugin::provider::AgentProvider;
use codeterm_types::pty::PendingPrompt;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

mod directory_trust;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Update,
    Onboarding,
    DirectoryTrust,
    Permission,
}

impl Kind {
    fn label_matches(self, label: &str) -> bool {
        match self {
            Self::Update => label == PROVIDER_UPDATE_PROMPT,
            Self::Onboarding => label == LOGIN_OR_AUTH_REQUIRED,
            Self::DirectoryTrust | Self::Permission => label == TRUST_OR_PERMISSION_PROMPT,
        }
    }
    fn sentinel_label_matches(self, label: &str) -> bool {
        match self {
            Self::Onboarding => label == TRUST_OR_PERMISSION_PROMPT,
            kind => kind.label_matches(label),
        }
    }
    pub(super) fn context(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Onboarding => "onboarding",
            Self::DirectoryTrust => "directory_trust",
            Self::Permission => "tool_permission",
        }
    }
    pub(super) fn prompt_kind(self) -> codeterm_types::pty::PromptKind {
        match self {
            Self::Update => codeterm_types::pty::PromptKind::ManualUpdate,
            Self::Onboarding => codeterm_types::pty::PromptKind::Provider,
            Self::DirectoryTrust => codeterm_types::pty::PromptKind::DirectoryTrust,
            Self::Permission => codeterm_types::pty::PromptKind::Provider,
        }
    }
    fn qualifying_label(self) -> &'static str {
        match self {
            Self::Update | Self::Onboarding => LOGIN_OR_AUTH_REQUIRED,
            Self::DirectoryTrust | Self::Permission => TRUST_OR_PERMISSION_PROMPT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub(super) kind: Kind,
    pub(super) prompt: PendingPrompt,
    pub(super) choice: String,
    pub(super) frame_key: String,
    pub(super) step: Vec<u8>,
}

fn unique_choice(prompt: &PendingPrompt, choice: &str) -> bool {
    prompt.multi_select != Some(true)
        && prompt.options.len() <= 10
        && prompt.options.len() >= 2
        && prompt
            .options
            .iter()
            .filter(|option| option.value == choice)
            .count()
            == 1
        && prompt
            .options
            .iter()
            .map(|option| &option.value)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == prompt.options.len()
}

pub(super) fn provider_target(provider: &dyn AgentProvider, screen: &str) -> Option<Target> {
    let prompt = provider.parse_prompt(screen)?;
    let (kind, choice, frame_key, step) =
        if let Some(response) = provider.launch_onboarding_response(screen) {
            (
                Kind::Onboarding,
                provider.launch_onboarding_safe_choice(screen)?,
                response.frame_key,
                response.step,
            )
        } else {
            let choice = provider.update_prompt_safe_choice(&prompt)?;
            let start = screen
                .rfind(prompt.question.as_str())
                .filter(|_| !prompt.question.is_empty())?;
            let modal = &screen[start..];
            if provider.parse_prompt(modal).as_ref() != Some(&prompt) {
                return None;
            }
            let index = prompt
                .options
                .iter()
                .position(|option| option.value == choice)?;
            let steps = provider.live_prompt_response(index, modal)?;
            let frame_key = format!(
                "update:{}",
                frame_hash(&serde_json::to_string(&steps).ok()?)
            );
            (Kind::Update, choice, frame_key, steps.first()?.clone())
        };
    (unique_choice(&prompt, &choice) && !step.is_empty()).then_some(Target {
        kind,
        prompt,
        choice,
        frame_key,
        step,
    })
}

pub(super) struct Evidence<'a> {
    pub(super) observations: usize,
    pub(super) confidence: f64,
    pub(super) qualified: bool,
    pub(super) current: bool,
    pub(super) eligible: bool,
    pub(super) model_choice: Option<(&'a str, f64)>,
}

pub(super) fn authorized(target: &Target, evidence: &Evidence<'_>) -> bool {
    evidence.observations >= 2
        && (0.90..=1.0).contains(&evidence.confidence)
        && evidence.qualified
        && evidence.current
        && evidence.eligible
        && unique_choice(&target.prompt, &target.choice)
        && !target.step.is_empty()
        && evidence.model_choice.is_some_and(|(choice, confidence)| {
            choice == target.choice && (0.90..=1.0).contains(&confidence)
        })
}

#[derive(Default)]
pub(super) struct Agreement {
    key: Option<(String, String, String)>,
    prior: Option<(u64, Instant)>,
    count: usize,
    minimum: f64,
    adapter: Option<std::sync::Weak<dyn DecisionModel>>,
}

impl Agreement {
    fn bind_adapter(&mut self, adapter: &Arc<dyn DecisionModel>) {
        if self
            .adapter
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_none_or(|prior| !Arc::ptr_eq(&prior, adapter))
        {
            self.key = None;
            self.prior = None;
            self.count = 0;
            self.minimum = 0.0;
        }
        self.adapter = Some(Arc::downgrade(adapter));
    }

    pub(super) fn observe(
        &mut self,
        launch: &str,
        occurrence: &str,
        observation: u64,
        choice: &str,
        confidence: f64,
        deadline: Instant,
    ) -> (usize, f64) {
        let key = (
            launch.to_string(),
            occurrence.to_string(),
            choice.to_string(),
        );
        if self.key.as_ref() != Some(&key) {
            self.prior = None;
            self.count = 0;
            self.minimum = 0.0;
        }
        self.key = Some(key);
        if self
            .prior
            .is_some_and(|(_, expires)| Instant::now() >= expires)
        {
            self.prior = None;
            self.count = 0;
            self.minimum = 0.0;
        }
        if !(0.90..=1.0).contains(&confidence) {
            self.prior = None;
            self.count = 0;
            self.minimum = 0.0;
            return (0, confidence);
        }
        if self.prior.is_some_and(|(prior, _)| prior >= observation) {
            return (self.count, self.minimum);
        }
        self.count = (self.count + 1).min(2);
        self.minimum = if self.count == 1 {
            confidence
        } else {
            self.minimum.min(confidence)
        };
        self.prior = Some((observation, deadline));
        (self.count, self.minimum)
    }
}

static AGREEMENT: LazyLock<Mutex<HashMap<String, Agreement>>> = LazyLock::new(Default::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    NotHandled,
    ConfirmationQueued,
    StepQueued,
}

pub(super) fn cancel_for_correlation(tab: &str, correlation: &identity::correlation::Correlation) {
    let mut agreements = AGREEMENT.lock();
    if agreements.get(tab).is_some_and(|agreement| {
        agreement
            .key
            .as_ref()
            .is_some_and(|(launch, occurrence, _)| {
                launch == correlation.launch_id() && occurrence == correlation.occurrence_id()
            })
    }) {
        agreements.remove(tab);
    }
}

pub(super) fn cancel_for_verdict(tab: &str, verdict: &CandidateVerdict) {
    if let Some(correlation) = verdict
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.metadata.envelope.as_ref())
        .map(|envelope| &envelope.correlation)
    {
        cancel_for_correlation(tab, correlation);
    }
}

#[cfg(test)]
static QUALIFICATION_OVERRIDES: LazyLock<
    Mutex<HashMap<(String, String, String), serde_json::Value>>,
> = LazyLock::new(Default::default);

#[cfg(test)]
pub(super) struct QualificationOverrideGuard((String, String, String));

#[cfg(test)]
impl Drop for QualificationOverrideGuard {
    fn drop(&mut self) {
        QUALIFICATION_OVERRIDES.lock().remove(&self.0);
    }
}

#[cfg(test)]
pub(super) fn override_qualification_for_test(
    adapter: &str,
    model: &str,
    criteria: &str,
    fixture: serde_json::Value,
) -> QualificationOverrideGuard {
    let key = (adapter.to_string(), model.to_string(), criteria.to_string());
    QUALIFICATION_OVERRIDES.lock().insert(key.clone(), fixture);
    QualificationOverrideGuard(key)
}

#[derive(Clone, Debug)]
pub(super) struct Receipt {
    pub(super) metadata: TabStateMetadata,
    pub(super) adapter_id: String,
    pub(super) ticket: coordinator::ActionTicket,
    pub(super) deadline: Instant,
}

impl Receipt {
    pub(super) fn current<H: ShadowHost>(
        &self,
        host: &H,
        tab: &str,
        ledgers: &coordinator::LedgerSnapshot,
    ) -> bool {
        let Some(envelope) = self.metadata.envelope.as_ref() else {
            return false;
        };
        Instant::now() < self.deadline
            && host.connection_is_current(&self.adapter_id)
            && envelope.adapter.as_ref().is_some_and(|adapter| {
                host.supporter_models()
                    .get(&self.adapter_id)
                    .is_some_and(|current| Arc::ptr_eq(adapter, &current))
            })
            && coordinator::global().is_current(&envelope.observation)
            && host.ledger_snapshot(tab) == *ledgers
            && host.observation_is_current(tab, &self.metadata)
    }
    fn unclaimed_mutation_current(&self, service: &ServiceContext, tab: &str) -> bool {
        let ledgers = service.ledger_snapshot(tab);
        coordinator::global().ticket_is_current(&self.ticket, &ledgers)
            && self.current(service, tab, &ledgers)
    }
}

pub(super) fn criteria_version() -> String {
    supporter::criteria_hash(
        &serde_json::to_vec(&serde_json::json!({
            "classification": criteria(),
            "prompt_visible": "current_interactive_prompt_noul_v1",
            "input_area": ["active_modal", "composer", "historical_or_draft_text", OTHER],
            "safe_choice": "unique_provider_option_values_plus_other_v1",
            "actions": [
                "provider_declared_update_skip",
                "delegated_provider_onboarding_not_now",
                "provider_declared_directory_trust_option"
            ],
            "minimum_component": 0.90,
            "observations": "two_distinct_completed_ids_in_current_occurrence_and_step_v1",
        }))
        .unwrap_or_default(),
    )
}

fn recorded_qualification(
) -> Option<super::super::general_agent::recovery::qualification::Qualification> {
    serde_json::from_str(include_str!(
        "../../../../evals/tab-state-jev/safe-response-qualification.json"
    ))
    .ok()
}

fn qualified_for(
    adapter: &str,
    model: Option<&str>,
    criteria: &str,
    heldout_hash: &str,
    label: &str,
) -> bool {
    #[cfg(test)]
    if model.is_some_and(|model| {
        QUALIFICATION_OVERRIDES
            .lock()
            .get(&(adapter.to_string(), model.to_string(), criteria.to_string()))
            .is_some_and(|fixture| fixture["fixture_authorized"] == true)
    }) {
        return true;
    }
    recorded_qualification().is_some_and(|qualification| {
        qualification.authorizes(adapter, model, criteria, heldout_hash)
            && qualification.supports_label(label)
    })
}

fn receipt_qualified(receipt: &Receipt, kind: Kind) -> bool {
    let model = receipt
        .metadata
        .envelope
        .as_ref()
        .and_then(|envelope| envelope.adapter.as_ref())
        .and_then(|model| model.model_id());
    let heldout_hash = supporter::criteria_hash(include_bytes!(
        "../../../../evals/tab-state-jev/heldout.jsonl"
    ));
    qualified_for(
        &receipt.adapter_id,
        model.as_deref(),
        &criteria_version(),
        &heldout_hash,
        kind.qualifying_label(),
    )
}

impl ServiceContext {
    pub(super) fn followup_scope(&self, tab_id: &str, screen: &str) -> Option<String> {
        let tab = self.mux.read().get_tab(tab_id)?.clone();
        let target = self.model_safe_response_target(tab_id, screen)?;
        self.followup_scope_for_target(tab_id, tab, target)
    }

    fn followup_scope_for_kind(&self, tab_id: &str, screen: &str, kind: Kind) -> Option<String> {
        let tab = self.mux.read().get_tab(tab_id)?.clone();
        let target = self.sentinel_safe_response_target(tab_id, screen, kind)?;
        self.followup_scope_for_target(tab_id, tab, target)
    }

    fn followup_scope_for_target(
        &self,
        tab_id: &str,
        tab: crate::multiplexer::types::Tab,
        target: Target,
    ) -> Option<String> {
        let prompt = identity::prompt_hash(&tab)?;
        if tab.pending_prompt.as_ref() != Some(&target.prompt)
            || tab.pending_prompt_kind != Some(target.kind.prompt_kind())
        {
            return None;
        }
        let config = self.supporter_decision_config();
        Some(supporter::criteria_hash(
            &serde_json::to_vec(&serde_json::json!([
                identity::LocalIdentity::of_tab(&tab).launch_key,
                prompt,
                target.frame_key,
                identity::supporter_policy(&config),
                self.ledger_snapshot(tab_id),
            ]))
            .ok()?,
        ))
    }

    pub(super) fn try_model_safe_response_from_receipt(
        &self,
        tab_id: &str,
        verdict: &CandidateVerdict,
    ) -> Progress {
        let Some(receipt) = verdict.receipt.as_ref() else {
            return Progress::NotHandled;
        };
        let Some(envelope) = receipt.metadata.envelope.as_ref() else {
            return Progress::NotHandled;
        };
        let Some(tab) = receipt.metadata.tab_snapshot.as_ref() else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        let Some(agent_type) = tab.agent_type.clone() else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        let snapshot = crate::agent::tab_state::TabStateSnapshot {
            tab_id: tab_id.to_string(),
            agent_type,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            screen_text: envelope.captured_screen.clone(),
        };
        if self.route_unattended_launch_prompt(&snapshot, verdict) {
            Progress::StepQueued
        } else {
            self.try_model_safe_response(&snapshot, verdict)
        }
    }

    pub(super) fn model_safe_response_target(&self, tab: &str, screen: &str) -> Option<Target> {
        let tab = self.mux.read().get_tab(tab)?.clone();
        if !crate::service::unattended_launch::owns_prompt_answers(&tab) {
            return None;
        }
        let provider = self
            .agent_registry
            .provider_arc(tab.agent_type.as_deref()?)?;
        let target = provider_target(provider.as_ref(), screen)
            .filter(|target| {
                target.kind == Kind::Update
                    || (target.kind == Kind::Onboarding
                        && directory_trust::onboarding_target_applies(&tab))
            })
            .or_else(|| {
                directory_trust::target_applies(&tab)
                    .then(|| directory_trust::provider_target(provider.as_ref(), screen))
                    .flatten()
            })
            .or_else(|| {
                let (_, signal) = self
                    .prompt_detector
                    .lock()
                    .pending_permission_request(&tab.id)?;
                let prompt = parse_prompt_safely(|| provider.parse_prompt(screen))?;
                directory_trust::permission_target(
                    provider.as_ref(),
                    screen,
                    prompt,
                    &signal.tool_name,
                )
            })?;
        let applies = match target.kind {
            Kind::Update => true,
            Kind::Onboarding => directory_trust::onboarding_target_applies(&tab),
            Kind::DirectoryTrust => directory_trust::target_applies(&tab),
            Kind::Permission => true,
        };
        applies.then_some(target)
    }

    pub(super) fn sentinel_safe_response_target(
        &self,
        tab_id: &str,
        screen: &str,
        kind: Kind,
    ) -> Option<Target> {
        let tab = self.mux.read().get_tab(tab_id)?.clone();
        if !crate::service::unattended_launch::owns_prompt_answers(&tab) {
            return None;
        }
        let provider = self
            .agent_registry
            .provider_arc(tab.agent_type.as_deref()?)?;
        let target = match kind {
            Kind::Update | Kind::Onboarding => {
                provider_target(provider.as_ref(), screen).filter(|target| target.kind == kind)
            }
            Kind::DirectoryTrust => directory_trust::target_applies(&tab)
                .then(|| directory_trust::provider_target(provider.as_ref(), screen))
                .flatten(),
            Kind::Permission => {
                let (_, signal) = self
                    .prompt_detector
                    .lock()
                    .pending_permission_request(tab_id)?;
                let prompt = parse_prompt_safely(|| provider.parse_prompt(screen))?;
                directory_trust::permission_target(
                    provider.as_ref(),
                    screen,
                    prompt,
                    &signal.tool_name,
                )
            }
        }?;
        (target.kind == kind
            && (kind != Kind::Onboarding || directory_trust::onboarding_target_applies(&tab)))
        .then_some(target)
    }

    pub(super) fn add_safe_response_metadata(
        &self,
        tab: &str,
        screen: &str,
        metadata: &mut TabStateMetadata,
    ) {
        let Some(target) = self.model_safe_response_target(tab, screen) else {
            return;
        };
        metadata.context["safe_response_kind"] = serde_json::json!(target.kind.context());
        metadata.declared_safe_choice = Some(target.choice);
        metadata.prompt_options = target
            .prompt
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| NumberedPromptOption {
                number: index + 1,
                label: option.label.clone(),
                value: option.value.clone(),
            })
            .collect();
    }

    pub(super) fn add_sentinel_safe_response_metadata(
        &self,
        tab: &str,
        screen: &str,
        metadata: &mut TabStateMetadata,
    ) {
        let target = [
            Kind::Update,
            Kind::Onboarding,
            Kind::DirectoryTrust,
            Kind::Permission,
        ]
        .into_iter()
        .find_map(|kind| self.sentinel_safe_response_target(tab, screen, kind));
        let Some(target) = target else {
            return;
        };
        metadata.context["safe_response_kind"] = serde_json::json!(target.kind.context());
        metadata.declared_safe_choice = Some(target.choice);
        metadata.prompt_options = target
            .prompt
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| NumberedPromptOption {
                number: index + 1,
                label: option.label.clone(),
                value: option.value.clone(),
            })
            .collect();
    }

    pub(crate) fn try_model_safe_response(
        &self,
        snapshot: &crate::agent::tab_state::TabStateSnapshot,
        verdict: &CandidateVerdict,
    ) -> Progress {
        self.try_model_safe_response_inner(snapshot, verdict, None, None)
    }

    pub(super) fn try_sentinel_safe_response(
        &self,
        snapshot: &crate::agent::tab_state::TabStateSnapshot,
        verdict: &CandidateVerdict,
        kind: Kind,
        requested_by: &str,
    ) -> Progress {
        self.try_model_safe_response_inner(snapshot, verdict, Some(kind), Some(requested_by))
    }

    fn try_model_safe_response_inner(
        &self,
        snapshot: &crate::agent::tab_state::TabStateSnapshot,
        verdict: &CandidateVerdict,
        requested_kind: Option<Kind>,
        requested_by: Option<&str>,
    ) -> Progress {
        let tab_id = &snapshot.tab_id;
        let Some(receipt) = verdict.receipt.as_ref() else {
            return Progress::NotHandled;
        };
        let Some(envelope) = receipt.metadata.envelope.as_ref() else {
            return Progress::NotHandled;
        };
        if !privacy::complete_screen(&snapshot.screen_text) {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        if envelope.correlation.decision_point() != "prompt_candidate"
            || !envelope.correlation.is_safe_response_producer()
        {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        let target = requested_kind
            .and_then(|kind| {
                self.sentinel_safe_response_target(tab_id, &snapshot.screen_text, kind)
            })
            .or_else(|| {
                requested_kind
                    .is_none()
                    .then(|| self.model_safe_response_target(tab_id, &snapshot.screen_text))
                    .flatten()
            });
        let Some(target) = target.filter(|target| {
            requested_kind.is_none_or(|kind| target.kind == kind)
                && if requested_kind.is_some() {
                    target.kind.sentinel_label_matches(&verdict.label)
                } else {
                    target.kind.label_matches(&verdict.label)
                }
        }) else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        if target.kind == Kind::Permission && requested_kind.is_none() {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        if !verdict.live_prompt()
            || receipt.metadata.declared_safe_choice.as_deref() != Some(&target.choice)
            || !candidates::allows_prompt(verdict, &snapshot.screen_text, &target.prompt)
            || (requested_kind != Some(Kind::Permission)
                && self.prompt_detector.lock().active_prompt_explicit(tab_id) == Some(true))
        {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        let choice = verdict
            .safe_choice
            .as_ref()
            .map(|(choice, confidence)| (choice.as_str(), *confidence));
        let confidence = choice
            .filter(|(choice, _)| *choice == target.choice)
            .map_or(0.0, |(_, confidence)| verdict.confidence.min(confidence));
        let Some(adapter) = envelope.adapter.as_ref() else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        let config = self.supporter_decision_config();
        let semantic = format!(
            "{:?}:{}:{}:{}:{}:{:?}:{:?}:{:?}:{:?}",
            target.kind,
            verdict.label,
            target.choice,
            target.frame_key,
            receipt.adapter_id,
            adapter.model_id(),
            envelope.policy_hash,
            envelope.local,
            envelope.ledgers
        );
        let tab = self.mux.read().get_tab(tab_id).cloned();
        let eligible = tab.as_ref().is_some_and(|tab| {
            tab.pending_prompt.as_ref() == Some(&target.prompt)
                && tab.pending_prompt_kind == Some(target.kind.prompt_kind())
        });
        if !eligible
            || !receipt.unclaimed_mutation_current(self, tab_id)
            || config.active_id() != Some(receipt.adapter_id.as_str())
        {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        let qualified = receipt_qualified(receipt, target.kind);
        let scope = requested_kind.map_or_else(
            || self.followup_scope(tab_id, &snapshot.screen_text),
            |kind| self.followup_scope_for_kind(tab_id, &snapshot.screen_text, kind),
        );
        let Some(scope) = scope else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        let (observations, minimum) = {
            let mut books = AGREEMENT.lock();
            let book = books.entry(tab_id.clone()).or_default();
            book.bind_adapter(adapter);
            book.observe(
                envelope.correlation.launch_id(),
                envelope.correlation.occurrence_id(),
                envelope.observation_id,
                &semantic,
                confidence,
                receipt.deadline,
            )
        };
        if observations == 1 {
            if !coordinator::global().claim_followup(tab_id, &scope, true) {
                cancel_for_correlation(tab_id, &envelope.correlation);
                return Progress::NotHandled;
            }
            let queued = if let (Some(kind), Some(requested_by)) = (requested_kind, requested_by) {
                let Some(local) = envelope.local.as_ref() else {
                    cancel_for_correlation(tab_id, &envelope.correlation);
                    return Progress::NotHandled;
                };
                self.observe_sentinel_prompt_candidate(
                    snapshot.clone(),
                    local,
                    kind,
                    requested_by,
                    Some(envelope.observation_id),
                )
                .is_some()
            } else if directory_trust::confirms_unattended(target.kind, tab.as_ref()) {
                self.observe_unattended_candidate(snapshot, envelope.local.as_ref())
            } else {
                self.observe_scrape_candidate_from(
                    snapshot.clone(),
                    |_| {},
                    ObservationTrigger::PromptConfirmation,
                    Some(envelope.observation_id),
                    envelope.local.as_ref(),
                )
            };
            if queued {
                return Progress::ConfirmationQueued;
            }
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        }
        if !authorized(
            &target,
            &Evidence {
                observations,
                confidence: minimum,
                qualified,
                current: receipt.unclaimed_mutation_current(self, tab_id)
                    && config.active_id() == Some(receipt.adapter_id.as_str()),
                eligible,
                model_choice: choice,
            },
        ) {
            return Progress::NotHandled;
        }
        let Some(pty) = tab.and_then(|tab| tab.pty_id) else {
            cancel_for_correlation(tab_id, &envelope.correlation);
            return Progress::NotHandled;
        };
        let episode = format!(
            "{}:{}",
            receipt.metadata.session_key, envelope.observation.occurrence
        );
        let occurrence = format!("{episode}:{}", target.frame_key);
        let trust_occurrence = target.frame_key.clone();
        let screen = snapshot.screen_text.clone();
        let claimed = match target.kind {
            Kind::Update => {
                super::super::output::claim_manual_update_occurrence(tab_id, &occurrence)
            }
            Kind::Onboarding => super::super::directory_trust::claim_model_onboarding_step(
                tab_id,
                &episode,
                &target.frame_key,
            ),
            Kind::DirectoryTrust => {
                super::super::directory_trust::claim_model_trust_step(tab_id, &trust_occurrence)
            }
            Kind::Permission => self
                .prompt_detector
                .lock()
                .pending_permission_request(tab_id)
                .is_some_and(|(instance, _)| {
                    super::super::permission_auto_confirm::claim_safe_prompt_response(
                        tab_id,
                        instance.id,
                        &target.prompt,
                    )
                }),
        };
        if !claimed {
            return Progress::NotHandled;
        }
        let ledgers = self.ledger_snapshot(tab_id);
        let owned_revision = match target.kind {
            Kind::Update => frame_hash(&occurrence),
            Kind::Onboarding => {
                super::super::directory_trust::model_onboarding_step_revision(&target.frame_key)
            }
            Kind::DirectoryTrust => {
                super::super::directory_trust::model_trust_step_revision(&trust_occurrence)
            }
            Kind::Permission => super::super::permission_auto_confirm::observation_revision(tab_id),
        };
        let mut unchanged = ledgers.clone();
        match target.kind {
            Kind::Update => unchanged.update = envelope.ledgers.update,
            Kind::Onboarding | Kind::DirectoryTrust => unchanged.trust = envelope.ledgers.trust,
            Kind::Permission => unchanged.permission = envelope.ledgers.permission,
        }
        let service = self.clone();
        let tab = tab_id.clone();
        let receipt = receipt.clone();
        let step = target.step.clone();
        let label = verdict.label.clone();
        let correlation = envelope.correlation.clone();
        let observation_id = envelope.observation_id;
        let requested_by = requested_by.map(str::to_owned);
        let trust_answer = (target.kind == Kind::DirectoryTrust).then(|| trust_occurrence.clone());
        let bound_pty = pty.clone();
        let agent = snapshot.agent_type.clone();
        let authorize = move || {
            let own_ledger_current = match target.kind {
                Kind::Update => ledgers.update == owned_revision,
                Kind::Onboarding | Kind::DirectoryTrust => ledgers.trust == owned_revision,
                Kind::Permission => ledgers.permission == owned_revision,
            };
            let current = own_ledger_current
                && unchanged == receipt.metadata.envelope.as_ref().unwrap().ledgers
                && receipt.current(&service, &tab, &ledgers)
                && requested_kind.map_or_else(
                    || service.model_safe_response_target(&tab, &screen).as_ref() == Some(&target),
                    |kind| {
                        service
                            .sentinel_safe_response_target(&tab, &screen, kind)
                            .as_ref()
                            == Some(&target)
                    },
                );
            let current = current
                && service.begin_automatic_prompt_answer(
                    &tab,
                    target.kind.context(),
                    service.prompt_chain_fingerprint(target.kind.prompt_kind(), &target.prompt),
                    None,
                );
            match target.kind {
                Kind::Onboarding => super::super::directory_trust::finish_model_onboarding_step(
                    &tab,
                    &episode,
                    &target.frame_key,
                    current,
                ),
                Kind::DirectoryTrust => super::super::directory_trust::finish_model_trust_step(
                    &tab,
                    &trust_occurrence,
                    current,
                ),
                Kind::Permission => {}
                Kind::Update if !current => {
                    super::super::output::release_model_update_step(&tab, &occurrence)
                }
                Kind::Update => {}
            }
            let mut event = identity::journal(
                &tab,
                identity::correlation::RecordIdentity::captured(observation_id, &correlation),
                identity::DecisionSource::Adapter,
                &label,
                Some(minimum),
                &receipt.metadata.title,
            );
            event["action"] = serde_json::json!("recovery");
            event["phase"] = serde_json::json!(if current {
                "action_authorized"
            } else {
                "action_stale"
            });
            event["reason"] = serde_json::json!(if current {
                "provider_safe_step_authorized"
            } else {
                "provider_safe_step_stale"
            });
            if let Some(requested_by) = requested_by.as_deref() {
                event["requested_by"] = serde_json::json!(requested_by);
            }
            let context = supporter::ActivityContext::TabState {
                title: receipt.metadata.title.clone(),
                declared_safe_choice: Some(target.choice.clone()),
                decision_point: "safe_response".into(),
            };
            let answers = BTreeMap::from([(
                "safe_choice".into(),
                DecisionAnswer::Choice {
                    choice: target.choice.clone(),
                    confidence: Some(minimum),
                    probabilities: BTreeMap::new(),
                },
            )]);
            event["asked"] = privacy::value(
                &serde_json::to_value(context.answers(&answers)).unwrap_or_default(),
            );
            let _ = service.supporter_runtime().decision(event);
            current
        };
        if let Some(occurrence) = trust_answer {
            self.queue_directory_trust_answer(tab_id, &bound_pty, &agent, &occurrence, authorize);
        } else {
            super::super::output::write_bound_prompt_steps(
                self.pty.clone(),
                pty,
                vec![step],
                authorize,
            );
        }
        Progress::StepQueued
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod qualified_tests;
