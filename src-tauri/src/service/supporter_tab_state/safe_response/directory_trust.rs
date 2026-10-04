use super::*;
use crate::agent::prompt::parse_prompt_safely;
use crate::agent::tab_state::TabStateSnapshot;
use crate::multiplexer::types::Tab;
use crate::service::general_agent::recovery::{
    AUTOMATIC_ACTION_CONFIDENCE, OWNER_NOTIFICATION_CONFIDENCE,
};
use crate::service::unattended_launch::{at_unattended_launch, runs_unattended};
use codeterm_plugin::provider::DirectoryTrustPromptResponse;
use codeterm_types::pty::PromptKind;

const NON_AFFIRMATIVE_CHOICE: &str = "non_affirmative_choice";
const UNDECLARED_CHOICE: &str = "undeclared_choice";

/// The trust key the provider declares advances toward exactly one of its own options;
/// that option is the only value the model may confirm.
pub(super) fn provider_target(provider: &dyn AgentProvider, screen: &str) -> Option<Target> {
    let prompt = parse_prompt_safely(|| provider.parse_directory_trust_prompt(screen))?;
    let Some(DirectoryTrustPromptResponse::Answer { step }) =
        provider.directory_trust_prompt_response(screen)
    else {
        return None;
    };
    let mut advanced = (0..prompt.options.len()).filter(|index| {
        provider
            .live_prompt_response(*index, screen)
            .and_then(|steps| steps.into_iter().next())
            .as_deref()
            == Some(step.as_slice())
    });
    let index = advanced.next()?;
    if advanced.next().is_some() {
        return None;
    }
    let choice = prompt.options[index].value.clone();
    let frame_key = format!(
        "directory-trust:{}",
        frame_hash(&serde_json::to_string(&(&prompt, &step)).ok()?)
    );
    (unique_choice(&prompt, &choice) && !step.is_empty()).then_some(Target {
        kind: Kind::DirectoryTrust,
        prompt,
        choice,
        frame_key,
        step,
    })
}

pub(super) fn target_applies(tab: &Tab) -> bool {
    at_unattended_launch(tab) && crate::service::directory_trust::directory_trust_check_applies(tab)
}

/// Unattended launch gates confirm through the output-candidate producer, whose
/// unanswered observation still reaches the deterministic fallback.
pub(super) fn confirms_unattended(kind: Kind, tab: Option<&Tab>) -> bool {
    kind != Kind::Update && tab.is_some_and(runs_unattended)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UnattendedRoute {
    Model,
    Fallback(&'static str),
    OwnerNotice(&'static str),
}

impl UnattendedRoute {
    fn vetoes(self) -> bool {
        matches!(
            self,
            Self::OwnerNotice(NON_AFFIRMATIVE_CHOICE | UNDECLARED_CHOICE)
        )
    }
}

pub(super) struct VerdictView<'a> {
    pub(super) label: &'a str,
    pub(super) live_prompt: bool,
    pub(super) confidence: f64,
    pub(super) safe_choice: Option<(&'a str, f64)>,
}

pub(super) fn route(
    target: Option<&Target>,
    verdict: &VerdictView<'_>,
    qualified: bool,
    vetoed: bool,
) -> UnattendedRoute {
    if vetoed {
        return UnattendedRoute::OwnerNotice("vetoed");
    }
    if let (Some(target), Some((choice, _))) = (target, verdict.safe_choice) {
        if choice != OTHER && choice != target.choice {
            let declared = target.prompt.options.iter().any(|o| o.value == choice);
            return UnattendedRoute::OwnerNotice(if declared {
                NON_AFFIRMATIVE_CHOICE
            } else {
                UNDECLARED_CHOICE
            });
        }
    }
    let Some(target) = target else {
        return UnattendedRoute::Fallback("no_candidate");
    };
    if !target.kind.label_matches(verdict.label) || !verdict.live_prompt {
        return if verdict.confidence >= OWNER_NOTIFICATION_CONFIDENCE {
            UnattendedRoute::OwnerNotice("model_disagrees")
        } else {
            UnattendedRoute::Fallback("low_confidence")
        };
    }
    let Some((_, choice_confidence)) = verdict
        .safe_choice
        .filter(|(choice, _)| *choice == target.choice)
    else {
        return UnattendedRoute::Fallback("abstain");
    };
    if ![verdict.confidence, choice_confidence]
        .iter()
        .all(|confidence| (AUTOMATIC_ACTION_CONFIDENCE..=1.0).contains(confidence))
    {
        return UnattendedRoute::Fallback("low_confidence");
    }
    if !qualified {
        return UnattendedRoute::Fallback("unqualified");
    }
    UnattendedRoute::Model
}

static VETOES: LazyLock<Mutex<HashMap<String, (String, u64)>>> = LazyLock::new(Default::default);

fn veto_key(tab: &Tab, prompt: &PendingPrompt) -> (String, u64) {
    (
        identity::LocalIdentity::of_tab(tab).launch_key,
        frame_hash(
            &serde_json::to_string(&(&prompt.question, &prompt.options)).unwrap_or_default(),
        ),
    )
}

pub(super) struct Gate {
    kind: Kind,
    prompt: Option<(PendingPrompt, PromptKind)>,
    target: Option<Target>,
    vetoed: bool,
    answers: bool,
}

impl ServiceContext {
    /// A provider-declared launch gate on a tab nobody watches: launch onboarding,
    /// or a folder-trust dialog before the launch session authenticated.
    fn unattended_launch_gate(&self, tab_id: &str, screen: &str) -> Option<Gate> {
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .filter(|tab| runs_unattended(tab))
            .cloned()?;
        let agent = tab.agent_type.clone()?;
        let provider = self.agent_registry.provider_arc(&agent)?;
        let onboarding =
            crate::service::directory_trust::launch_onboarding_check_applies(&tab, &agent)
                && parse_prompt_safely(|| provider.launch_onboarding_response(screen)).is_some();
        let kind = if onboarding {
            Kind::Onboarding
        } else if !tab.launch_session_authenticated()
            && (parse_prompt_safely(|| provider.parse_directory_trust_prompt(screen)).is_some()
                || self.provider_answers_directory_trust(&agent, screen))
        {
            Kind::DirectoryTrust
        } else {
            return None;
        };
        let prompt =
            parse_prompt_safely(|| self.agent_registry.parse_interactive_prompt(&agent, screen))
                .filter(|(prompt, kind)| {
                    *kind == PromptKind::DirectoryTrust
                        || !crate::agent::state_reactor::prompt_is_composer_text(screen, prompt)
                });
        let answers = kind == Kind::Onboarding
            || matches!(
                provider.directory_trust_prompt_response(screen),
                Some(DirectoryTrustPromptResponse::Answer { .. })
            );
        let vetoed = prompt
            .as_ref()
            .is_some_and(|(prompt, _)| VETOES.lock().get(tab_id) == Some(&veto_key(&tab, prompt)));
        let target = self
            .model_safe_response_target(tab_id, screen)
            .filter(|target| target.kind == kind);
        Some(Gate {
            kind,
            prompt,
            target,
            vetoed,
            answers,
        })
    }

    /// Model-first decision for an unattended launch gate; `false` leaves the
    /// candidate to the ordinary publication path.
    pub(crate) fn route_unattended_launch_prompt(
        &self,
        snapshot: &TabStateSnapshot,
        verdict: &CandidateVerdict,
    ) -> bool {
        let Some(gate) = self.unattended_launch_gate(&snapshot.tab_id, &snapshot.screen_text)
        else {
            return false;
        };
        let qualified = verdict
            .receipt
            .as_ref()
            .zip(gate.target.as_ref())
            .is_some_and(|(receipt, target)| receipt_qualified(receipt, target.kind));
        let decision = route(
            gate.target.as_ref(),
            &VerdictView {
                label: &verdict.label,
                live_prompt: verdict.live_prompt(),
                confidence: verdict.confidence,
                safe_choice: verdict
                    .safe_choice
                    .as_ref()
                    .map(|(choice, confidence)| (choice.as_str(), *confidence)),
            },
            qualified,
            gate.vetoed,
        );
        self.journal_unattended_route(&snapshot.tab_id, Some(verdict), gate.kind, decision);
        match decision {
            UnattendedRoute::Model => self.advance_unattended_model(snapshot, verdict, &gate),
            UnattendedRoute::Fallback(_) => self.resolve_unattended_gate(snapshot, &gate),
            UnattendedRoute::OwnerNotice(_) => {
                if let Some(envelope) = verdict
                    .receipt
                    .as_ref()
                    .and_then(|receipt| receipt.metadata.envelope.as_ref())
                {
                    cancel_for_correlation(&snapshot.tab_id, &envelope.correlation);
                }
                if decision.vetoes() {
                    self.veto_unattended_prompt(&snapshot.tab_id, &gate);
                }
                self.notify_unattended_owner(snapshot, &gate);
            }
        }
        true
    }

    /// The fallback for an output candidate the model left unanswered.
    pub(crate) fn resolve_scrape_candidate_fallback(&self, snapshot: &TabStateSnapshot) {
        match self.unattended_launch_gate(&snapshot.tab_id, &snapshot.screen_text) {
            Some(gate) => self.resolve_unattended_gate(snapshot, &gate),
            None => self.resolve_scrape_candidate_heuristically(snapshot),
        }
    }

    /// A launch gate whose observation could not be queued still owes a decision.
    pub(crate) fn resolve_unobserved_launch_gate(&self, snapshot: &TabStateSnapshot) {
        let Some(gate) = self.unattended_launch_gate(&snapshot.tab_id, &snapshot.screen_text)
        else {
            return;
        };
        if self
            .prompt_detector
            .lock()
            .active_prompt_explicit(&snapshot.tab_id)
            == Some(true)
        {
            return;
        }
        self.journal_unattended_route(
            &snapshot.tab_id,
            None,
            gate.kind,
            UnattendedRoute::Fallback("unavailable"),
        );
        self.resolve_unattended_gate(snapshot, &gate);
    }

    pub(super) fn observe_unattended_candidate(
        &self,
        snapshot: &TabStateSnapshot,
        local: Option<&identity::LocalIdentity>,
    ) -> bool {
        let fallback = snapshot.clone();
        let queued = self.observe_scrape_candidate_from(
            snapshot.clone(),
            move |service| service.resolve_scrape_candidate_fallback(&fallback),
            ObservationTrigger::OutputCandidate,
            None,
            local,
        );
        if !queued {
            self.resolve_unobserved_launch_gate(snapshot);
        }
        queued
    }

    fn advance_unattended_model(
        &self,
        snapshot: &TabStateSnapshot,
        verdict: &CandidateVerdict,
        gate: &Gate,
    ) {
        let Some(target) = gate.target.as_ref() else {
            return;
        };
        let kind = target.kind.prompt_kind();
        let established = self
            .mux
            .read()
            .get_tab(&snapshot.tab_id)
            .is_some_and(|tab| {
                tab.pending_prompt.as_ref() == Some(&target.prompt)
                    && tab.pending_prompt_kind == Some(kind)
            });
        if established {
            self.try_model_safe_response(snapshot, verdict);
            return;
        }
        if !self.publish_unattended_launch_prompt(
            &snapshot.tab_id,
            &snapshot.screen_text,
            target.prompt.clone(),
            kind,
            false,
        ) {
            self.journal_unattended_route(
                &snapshot.tab_id,
                None,
                gate.kind,
                UnattendedRoute::Fallback("unpublished"),
            );
            self.resolve_unattended_gate(snapshot, gate);
            return;
        }
        let Some(scope) = self.followup_scope(&snapshot.tab_id, &snapshot.screen_text) else {
            return;
        };
        if coordinator::global().claim_followup(&snapshot.tab_id, &scope, false) {
            self.observe_unattended_candidate(snapshot, None);
        }
    }

    /// The deterministic responder decides under its own provenance and provider
    /// hook; whatever it will not answer goes to the owner once.
    fn resolve_unattended_gate(&self, snapshot: &TabStateSnapshot, gate: &Gate) {
        if gate.vetoed {
            self.notify_unattended_owner(snapshot, gate);
            return;
        }
        self.resolve_scrape_candidate_heuristically(snapshot);
        if !gate.answers {
            self.notify_unattended_owner(snapshot, gate);
        }
    }

    fn notify_unattended_owner(&self, snapshot: &TabStateSnapshot, gate: &Gate) {
        if let Some((prompt, kind)) = gate.prompt.clone() {
            self.publish_unattended_launch_prompt(
                &snapshot.tab_id,
                &snapshot.screen_text,
                prompt,
                kind,
                true,
            );
        }
    }

    fn veto_unattended_prompt(&self, tab_id: &str, gate: &Gate) {
        let tab = self.mux.read().get_tab(tab_id).cloned();
        if let Some((tab, (prompt, _))) = tab.zip(gate.prompt.as_ref()) {
            VETOES.lock().insert(tab_id.into(), veto_key(&tab, prompt));
        }
    }

    fn journal_unattended_route(
        &self,
        tab: &str,
        verdict: Option<&CandidateVerdict>,
        kind: Kind,
        decision: UnattendedRoute,
    ) {
        let receipt = verdict.and_then(|verdict| verdict.receipt.as_ref());
        let envelope = receipt.and_then(|receipt| receipt.metadata.envelope.as_ref());
        let observation = envelope.map_or_else(
            || identity::correlation::RecordIdentity::from(state::next_observation_id()),
            |envelope| {
                identity::correlation::RecordIdentity::captured(
                    envelope.observation_id,
                    &envelope.correlation,
                )
            },
        );
        let source = if verdict.is_some() {
            identity::DecisionSource::Adapter
        } else {
            identity::DecisionSource::Error
        };
        let title = receipt.map_or(tab, |receipt| receipt.metadata.title.as_str());
        let mut event = identity::journal(
            tab,
            observation,
            source,
            verdict.map_or(OTHER, |verdict| verdict.label.as_str()),
            verdict.map(|verdict| verdict.confidence),
            title,
        );
        let (phase, decided_by, reason) = match decision {
            UnattendedRoute::Model => {
                ("unattended_model", "adapter".to_string(), "declared_choice")
            }
            UnattendedRoute::OwnerNotice(reason) => {
                ("unattended_owner_notice", "adapter".to_string(), reason)
            }
            UnattendedRoute::Fallback(reason) => {
                ("unattended_fallback", format!("fallback({reason})"), reason)
            }
        };
        event["action"] = serde_json::json!("unattended_launch_prompt");
        event["phase"] = serde_json::json!(phase);
        event["decided_by"] = serde_json::json!(decided_by);
        event["reason"] = serde_json::json!(reason);
        event["prompt_kind"] = serde_json::json!(kind.context());
        let _ = self.supporter_runtime().decision(event);
    }
}

#[cfg(test)]
mod tests;
