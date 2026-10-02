use super::*;
use codeterm_plugin::provider::AgentProvider;
use codeterm_types::pty::PendingPrompt;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Update,
    Onboarding,
}

impl Kind {
    fn label_matches(self, label: &str) -> bool {
        match self {
            Self::Update => label == PROVIDER_UPDATE_PROMPT,
            Self::Onboarding => matches!(label, LOGIN_OR_AUTH_REQUIRED | STILL_STARTING),
        }
    }
    fn context(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Onboarding => "onboarding",
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
    key: Option<(String, u64, String)>,
    prior: Option<(u64, u64, f64)>,
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
        }
        self.adapter = Some(Arc::downgrade(adapter));
    }

    pub(super) fn observe(
        &mut self,
        launch: &str,
        occurrence: u64,
        observation: u64,
        frame: u64,
        choice: &str,
        confidence: f64,
    ) -> (usize, f64) {
        let key = (launch.to_string(), occurrence, choice.to_string());
        if self.key.as_ref() != Some(&key) {
            self.prior = None;
        }
        self.key = Some(key);
        if !(0.90..=1.0).contains(&confidence) {
            self.prior = None;
            return (0, confidence);
        }
        let result = match self.prior {
            Some((prior, old_frame, old_confidence))
                if prior >= observation || old_frame == frame =>
            {
                return (1, old_confidence)
            }
            Some((_, _, old_confidence)) => (2, old_confidence.min(confidence)),
            None => (1, confidence),
        };
        self.prior = Some((observation, frame, confidence));
        result
    }
}

static AGREEMENT: LazyLock<Mutex<HashMap<String, Agreement>>> = LazyLock::new(Default::default);

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
            "actions": ["provider_declared_update_skip", "delegated_provider_onboarding_not_now"],
            "minimum_component": 0.90,
            "observations": "two_distinct_ids_and_frames_in_current_occurrence_v1",
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

impl ServiceContext {
    pub(super) fn model_safe_response_target(&self, tab: &str, screen: &str) -> Option<Target> {
        let tab = self.mux.read().get_tab(tab)?.clone();
        let provider = self
            .agent_registry
            .provider_arc(tab.agent_type.as_deref()?)?;
        let target = provider_target(provider.as_ref(), screen)?;
        if target.kind == Kind::Onboarding
            && !super::super::directory_trust::launch_onboarding_check_applies(&tab, provider.id())
        {
            return None;
        }
        Some(target)
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

    pub(crate) fn try_model_safe_response(
        &self,
        snapshot: &crate::agent::tab_state::TabStateSnapshot,
        verdict: &CandidateVerdict,
    ) -> bool {
        let tab_id = &snapshot.tab_id;
        if !privacy::complete_screen(&snapshot.screen_text) {
            AGREEMENT.lock().remove(tab_id);
            return false;
        }
        let Some(receipt) = verdict.receipt.as_ref() else {
            return false;
        };
        let Some(envelope) = receipt.metadata.envelope.as_ref() else {
            return false;
        };
        let Some(target) = self
            .model_safe_response_target(tab_id, &snapshot.screen_text)
            .filter(|target| target.kind.label_matches(&verdict.label))
        else {
            AGREEMENT.lock().remove(tab_id);
            return false;
        };
        if !verdict.live_prompt()
            || receipt.metadata.declared_safe_choice.as_deref() != Some(&target.choice)
            || !candidates::allows_prompt(verdict, &snapshot.screen_text, &target.prompt)
            || self.prompt_detector.lock().active_prompt_explicit(tab_id) == Some(true)
        {
            AGREEMENT.lock().remove(tab_id);
            return false;
        }
        let choice = verdict
            .safe_choice
            .as_ref()
            .map(|(choice, confidence)| (choice.as_str(), *confidence));
        let confidence = choice
            .filter(|(choice, _)| *choice == target.choice)
            .map_or(0.0, |(_, confidence)| verdict.confidence.min(confidence));
        let Some(adapter) = envelope.adapter.as_ref() else {
            return false;
        };
        let semantic = format!(
            "{:?}:{}:{}:{}:{:?}:{:?}:{:?}",
            target.kind,
            verdict.label,
            target.choice,
            receipt.adapter_id,
            adapter.model_id(),
            envelope.policy_hash,
            envelope.local
        );
        let (observations, minimum) = {
            let mut books = AGREEMENT.lock();
            let book = books.entry(tab_id.clone()).or_default();
            book.bind_adapter(adapter);
            book.observe(
                &receipt.metadata.session_key,
                envelope.observation.occurrence,
                envelope.observation_id,
                envelope.frame_hash,
                &semantic,
                confidence,
            )
        };
        let tab = self.mux.read().get_tab(tab_id).cloned();
        let eligible = tab.as_ref().is_some_and(|tab| {
            tab.pending_prompt.as_ref() == Some(&target.prompt)
                && tab.pending_prompt_kind
                    == Some(match target.kind {
                        Kind::Update => codeterm_types::pty::PromptKind::ManualUpdate,
                        Kind::Onboarding => codeterm_types::pty::PromptKind::Provider,
                    })
        });
        let config = crate::config::load_config().supporter;
        let model = envelope.adapter.as_ref().and_then(|model| model.model_id());
        let qualified = recorded_qualification().is_some_and(|qualification| {
            qualification.authorizes(
                &receipt.adapter_id,
                model.as_deref(),
                &criteria_version(),
                &supporter::criteria_hash(include_bytes!(
                    "../../../../evals/tab-state-jev/heldout.jsonl"
                )),
            ) && qualification.supports_label(LOGIN_OR_AUTH_REQUIRED)
        });
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
            return false;
        }
        let Some(pty) = tab.and_then(|tab| tab.pty_id) else {
            return false;
        };
        let episode = format!(
            "{}:{}",
            receipt.metadata.session_key, envelope.observation.occurrence
        );
        let occurrence = format!("{episode}:{}", target.frame_key);
        let claimed = match target.kind {
            Kind::Update => {
                super::super::output::claim_manual_update_occurrence(tab_id, &occurrence)
            }
            Kind::Onboarding => super::super::directory_trust::claim_model_onboarding_step(
                tab_id,
                &episode,
                &target.frame_key,
            ),
        };
        if !claimed {
            return false;
        }
        let ledgers = self.ledger_snapshot(tab_id);
        let owned_revision = match target.kind {
            Kind::Update => frame_hash(&occurrence),
            Kind::Onboarding => {
                super::super::directory_trust::model_onboarding_step_revision(&target.frame_key)
            }
        };
        let mut unchanged = ledgers.clone();
        match target.kind {
            Kind::Update => unchanged.update = envelope.ledgers.update,
            Kind::Onboarding => unchanged.trust = envelope.ledgers.trust,
        }
        let service = self.clone();
        let tab = tab_id.clone();
        let screen = snapshot.screen_text.clone();
        let receipt = receipt.clone();
        let step = target.step.clone();
        let label = verdict.label.clone();
        super::super::output::write_bound_prompt_steps(
            self.pty.clone(),
            pty,
            vec![step],
            move || {
                let own_ledger_current = match target.kind {
                    Kind::Update => ledgers.update == owned_revision,
                    Kind::Onboarding => ledgers.trust == owned_revision,
                };
                let current = own_ledger_current
                    && unchanged == receipt.metadata.envelope.as_ref().unwrap().ledgers
                    && receipt.current(&service, &tab, &ledgers)
                    && service.model_safe_response_target(&tab, &screen).as_ref() == Some(&target);
                if target.kind == Kind::Onboarding {
                    super::super::directory_trust::finish_model_onboarding_step(
                        &tab,
                        &episode,
                        &target.frame_key,
                        current,
                    );
                } else if !current {
                    super::super::output::release_model_update_step(&tab, &occurrence);
                }
                let envelope = receipt.metadata.envelope.as_ref().unwrap();
                let mut event = identity::journal(
                    &tab,
                    envelope.observation_id,
                    identity::DecisionSource::Adapter,
                    &label,
                    Some(minimum),
                    &receipt.metadata.title,
                );
                event["action"] = serde_json::json!("recovery");
                event["reason"] = serde_json::json!(if current {
                    "provider_safe_step_authorized"
                } else {
                    "provider_safe_step_stale"
                });
                let context = supporter::ActivityContext::TabState {
                    title: receipt.metadata.title.clone(),
                    declared_safe_choice: Some(target.choice.clone()),
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
            },
        );
        true
    }
}

#[cfg(test)]
mod tests;
