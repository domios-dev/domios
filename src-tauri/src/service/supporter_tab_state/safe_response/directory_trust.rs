use super::*;
use crate::agent::prompt::parse_prompt_safely;
use crate::agent::tab_state::TabStateSnapshot;
use crate::multiplexer::types::{Tab, TabAudience};
use crate::service::unattended_launch::{at_owned_launch_prompt, owns_prompt_answers};
use codeterm_plugin::provider::DirectoryTrustPromptResponse;
use codeterm_types::pty::PromptKind;

/// The trust responder's key identifies the provider-declared option for the structural fallback.
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
    let frame_key = crate::service::directory_trust::directory_trust_occurrence(&prompt);
    (unique_choice(&prompt, &choice) && !step.is_empty()).then_some(Target {
        kind: Kind::DirectoryTrust,
        prompt,
        choice,
        frame_key,
        step,
    })
}

/// Unattended launch gates confirm through the output-candidate producer.
pub(super) fn confirms_unattended(kind: Kind, tab: Option<&Tab>) -> bool {
    kind != Kind::Update && tab.is_some_and(target_applies)
}

pub(super) fn target_applies(tab: &Tab) -> bool {
    at_owned_launch_prompt(tab) || tab.audience == TabAudience::UserOwnedAutomation
}

pub(super) fn onboarding_target_applies(tab: &Tab) -> bool {
    tab.agent_type.as_deref().is_some_and(|agent_type| {
        crate::service::directory_trust::launch_onboarding_check_applies(tab, agent_type)
    })
}

pub(super) fn permission_target(
    provider: &dyn AgentProvider,
    screen: &str,
    prompt: PendingPrompt,
    tool_name: &str,
) -> Option<Target> {
    let response = provider.permission_prompt_response(tool_name, &prompt, screen)?;
    let choice = prompt.options.get(response.option_index)?.value.clone();
    let step = response.steps.first()?.clone();
    let frame_key = format!(
        "tool-permission:{}",
        frame_hash(&serde_json::to_string(&(&prompt, &step)).ok()?)
    );
    (unique_choice(&prompt, &choice) && !step.is_empty()).then_some(Target {
        kind: Kind::Permission,
        prompt,
        choice,
        frame_key,
        step,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UnattendedRoute {
    Model,
    Fallback(&'static str),
}

pub(super) struct VerdictView<'a> {
    pub(super) label: &'a str,
    pub(super) live_prompt: bool,
    pub(super) confidence: f64,
    pub(super) safe_choice: Option<(&'a str, f64)>,
}

pub(super) fn route(target: Option<&Target>, verdict: &VerdictView<'_>) -> UnattendedRoute {
    let Some(target) = target else {
        return UnattendedRoute::Fallback("no_candidate");
    };
    if !target.kind.label_matches(verdict.label) || !verdict.live_prompt {
        return UnattendedRoute::Fallback("not_prompt");
    }
    let Some((choice, choice_confidence)) = verdict.safe_choice.filter(|(choice, _)| {
        *choice != OTHER
            && target
                .prompt
                .options
                .iter()
                .any(|option| option.value == *choice)
    }) else {
        return UnattendedRoute::Fallback("abstain");
    };
    if !target
        .prompt
        .options
        .iter()
        .any(|option| option.value == choice)
    {
        return UnattendedRoute::Fallback("undeclared_choice");
    }
    if ![verdict.confidence, choice_confidence]
        .iter()
        .all(|confidence| (0.80..=1.0).contains(confidence))
    {
        return UnattendedRoute::Fallback("low_confidence");
    }
    UnattendedRoute::Model
}

pub(super) struct Gate {
    kind: Kind,
    prompt: Option<(PendingPrompt, PromptKind)>,
    target: Option<Target>,
}

impl ServiceContext {
    fn unattended_launch_gate(&self, tab_id: &str, screen: &str) -> Option<Gate> {
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .filter(|tab| owns_prompt_answers(tab))
            .cloned()?;
        let agent = tab.agent_type.clone()?;
        let provider = self.agent_registry.provider_arc(&agent)?;
        let target = self.model_safe_response_target(tab_id, screen);
        let parsed =
            parse_prompt_safely(|| self.agent_registry.parse_interactive_prompt(&agent, screen))
                .filter(|(prompt, kind)| {
                    *kind == PromptKind::DirectoryTrust
                        || !crate::agent::state_reactor::prompt_is_composer_text(screen, prompt)
                });
        let kind = target.as_ref().map(|target| target.kind).or_else(|| {
            if !tab.launch_session_authenticated()
                && parse_prompt_safely(|| provider.parse_directory_trust_prompt(screen)).is_some()
            {
                Some(Kind::DirectoryTrust)
            } else if self
                .prompt_detector
                .lock()
                .pending_permission_request(tab_id)
                .is_some()
            {
                Some(Kind::Permission)
            } else {
                None
            }
        })?;
        let prompt = target
            .as_ref()
            .map(|target| (target.prompt.clone(), target.kind.prompt_kind()))
            .or(parsed);
        Some(Gate {
            kind,
            prompt,
            target,
        })
    }

    /// Model-first decision for an owned prompt; `false` leaves other prompts alone.
    pub(crate) fn route_unattended_launch_prompt(
        &self,
        snapshot: &TabStateSnapshot,
        verdict: &CandidateVerdict,
    ) -> bool {
        let Some(gate) = self.unattended_launch_gate(&snapshot.tab_id, &snapshot.screen_text)
        else {
            return false;
        };
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
        );
        self.journal_unattended_route(&snapshot.tab_id, Some(verdict), gate.kind, decision);
        match decision {
            UnattendedRoute::Model => self.advance_unattended_model(snapshot, verdict, &gate),
            UnattendedRoute::Fallback(_) => self.resolve_unattended_gate(snapshot, &gate),
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

    /// An owned prompt whose observation could not be queued still owes a decision.
    pub(crate) fn resolve_unobserved_launch_gate(&self, snapshot: &TabStateSnapshot) {
        let Some(gate) = self.unattended_launch_gate(&snapshot.tab_id, &snapshot.screen_text)
        else {
            self.resolve_scrape_candidate_heuristically(snapshot);
            return;
        };
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
            self.resolve_unattended_gate(snapshot, gate);
            return;
        };
        let Some((choice, _)) = verdict.safe_choice.as_ref() else {
            self.resolve_unattended_gate(snapshot, gate);
            return;
        };
        let Some(option_index) = target
            .prompt
            .options
            .iter()
            .position(|option| option.value == *choice)
        else {
            self.resolve_unattended_gate(snapshot, gate);
            return;
        };
        if target.kind == Kind::DirectoryTrust && *choice == target.choice {
            let occurrence = target.frame_key.clone();
            if !crate::service::directory_trust::claim_model_trust_step(
                &snapshot.tab_id,
                &occurrence,
            ) {
                return;
            }
            let pty = self
                .mux
                .read()
                .get_tab(&snapshot.tab_id)
                .and_then(|tab| tab.pty_id.clone());
            let Some(pty) = pty else {
                crate::service::directory_trust::finish_model_trust_step(
                    &snapshot.tab_id,
                    &occurrence,
                    false,
                );
                return;
            };
            let service = self.clone();
            let tab = snapshot.tab_id.clone();
            let expected = target.clone();
            let screen = snapshot.screen_text.clone();
            self.queue_directory_trust_answer(
                &snapshot.tab_id,
                &pty,
                &snapshot.agent_type,
                &occurrence,
                move || {
                    service.model_safe_response_target(&tab, &screen).as_ref() == Some(&expected)
                        && service.begin_automatic_prompt_answer(
                            &tab,
                            "directory_trust",
                            expected.frame_key,
                            None,
                        )
                },
            );
            return;
        }
        let kind = target.kind.prompt_kind();
        let identity = codeterm_types::pty::PromptIdentity::new(kind, &target.prompt);
        let chain_fingerprint = self.prompt_chain_fingerprint(kind, &target.prompt);
        if !self.begin_automatic_prompt_answer(
            &snapshot.tab_id,
            target.kind.context(),
            chain_fingerprint,
            None,
        ) {
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
        if target.kind == Kind::Update {
            super::super::super::provider_update::manual_prompt_observed(self, snapshot);
        }
        let result = self.respond_prompt_with_identity(
            &snapshot.tab_id,
            &(option_index + 1).to_string(),
            &identity,
        );
        if matches!(
            result,
            Ok(crate::service::tab::RespondPromptOutcome::Sent)
                | Ok(crate::service::tab::RespondPromptOutcome::Structured)
        ) {
            if target.kind == Kind::Update && *choice == target.choice {
                super::super::super::provider_update::record_safe_prompt_answer(
                    &snapshot.agent_type,
                    &snapshot.tab_id,
                );
            }
            return;
        }
        self.journal_unattended_route(
            &snapshot.tab_id,
            Some(verdict),
            gate.kind,
            UnattendedRoute::Fallback("response_failed"),
        );
        self.resolve_unattended_gate(snapshot, gate);
    }

    /// The deterministic responder decides under its own provenance and provider
    /// hook; whatever it will not answer goes to the owner once.
    fn resolve_unattended_gate(&self, snapshot: &TabStateSnapshot, gate: &Gate) {
        self.resolve_scrape_candidate_heuristically(snapshot);
        let unresolved = gate.prompt.as_ref().is_some_and(|(prompt, kind)| {
            self.mux
                .read()
                .get_tab(&snapshot.tab_id)
                .is_some_and(|tab| {
                    tab.pending_prompt.as_ref() == Some(prompt)
                        && tab.pending_prompt_kind == Some(*kind)
                })
        });
        if unresolved {
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
            identity::DecisionSource::unavailable()
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
            UnattendedRoute::Model => ("owned_model", "adapter".to_string(), "declared_choice"),
            UnattendedRoute::Fallback(reason) => {
                ("owned_fallback", format!("fallback({reason})"), reason)
            }
        };
        event["action"] = serde_json::json!("owned_prompt_answer");
        event["phase"] = serde_json::json!(phase);
        event["decided_by"] = serde_json::json!(decided_by);
        event["reason"] = serde_json::json!(reason);
        event["prompt_kind"] = serde_json::json!(kind.context());
        let _ = self.supporter_runtime().decision(event);
    }
}

#[cfg(test)]
mod tests;
