use super::*;
use codeterm_types::decision::DecisionError;

#[derive(Debug, Clone)]
pub(crate) struct CandidateVerdict {
    pub(crate) label: String,
    pub(crate) confidence: f64,
    visible: bool,
    area: String,
    pub(super) safe_choice: Option<(String, f64)>,
    pub(super) receipt: Option<safe_response::Receipt>,
}

impl CandidateVerdict {
    pub(crate) fn live_prompt(&self) -> bool {
        self.visible && self.area == "active_modal"
    }

    pub(crate) fn observation_id(&self) -> Option<u64> {
        self.receipt
            .as_ref()
            .and_then(|receipt| receipt.metadata.envelope.as_ref())
            .map(|envelope| envelope.observation_id)
    }
}

pub(crate) fn select(
    response: Result<DecisionResponse, DecisionError>,
) -> (Option<CandidateVerdict>, identity::DecisionSource) {
    let parsed = response.and_then(|response| {
        let area = response.answers.get("input_area").cloned();
        let visible_probability = match response.answers.get("prompt_visible") {
            Some(DecisionAnswer::Noul { p }) => *p,
            _ => return Err(DecisionError::Parse("invalid_prompt_visible".into())),
        };
        let verdict = observer::observation_answers(response)
            .map_err(|_| DecisionError::Parse("invalid_candidate".into()))?;
        let Some(DecisionAnswer::Choice {
            choice,
            probabilities,
            confidence,
        }) = area
        else {
            return Err(DecisionError::Parse("invalid_input_area".into()));
        };
        if ![
            "active_modal",
            "composer",
            "historical_or_draft_text",
            OTHER,
        ]
        .contains(&choice.as_str())
        {
            return Err(DecisionError::Parse("unknown_input_area".into()));
        }
        let confidence = confidence
            .or_else(|| probabilities.get(&choice).copied())
            .filter(|confidence| confidence.is_finite() && (0.0..=1.0).contains(confidence))
            .ok_or_else(|| DecisionError::Parse("invalid_input_confidence".into()))?;
        let visible = visible_probability >= 0.5;
        let minimum = verdict.confidence.min(confidence).min(if visible {
            visible_probability
        } else {
            1.0 - visible_probability
        });
        let other = verdict.label == OTHER || choice == OTHER;
        Ok((
            Some(CandidateVerdict {
                label: verdict.label,
                confidence: minimum,
                visible,
                area: choice,
                safe_choice: verdict.safe_choice,
                receipt: None,
            }),
            minimum,
            other,
        ))
    });
    identity::select(Some(parsed), || None)
}

pub(crate) fn allows_prompt(
    verdict: &CandidateVerdict,
    screen: &str,
    prompt: &crate::multiplexer::types::PendingPrompt,
) -> bool {
    verdict.live_prompt()
        && matches!(
            verdict.label.as_str(),
            TRUST_OR_PERMISSION_PROMPT
                | PROVIDER_UPDATE_PROMPT
                | LOGIN_OR_AUTH_REQUIRED
                | MODEL_UNAVAILABLE_OR_QUOTA
                | STILL_STARTING
        )
        && crate::agent::state_reactor::has_structural_prompt_evidence(screen, prompt)
        && !crate::agent::state_reactor::prompt_is_composer_text(screen, prompt)
}

struct Pending<H: ShadowHost> {
    host: H,
    tab: String,
    metadata: TabStateMetadata,
    source: identity::DecisionSource,
    recorded: bool,
    model_verdict: Option<identity::correlation::ModelVerdict>,
}

pub(super) fn receipt_authorization_deadline(captured_at: Instant, configured_ms: u64) -> Instant {
    captured_at + std::time::Duration::from_millis(configured_ms.max(1)).min(DEADLINE)
}

impl<H: ShadowHost> Pending<H> {
    fn current(&self, adapter_id: &str, adapter: &Arc<dyn DecisionModel>) -> bool {
        let Some(envelope) = self.metadata.envelope.as_ref() else {
            return false;
        };
        macro_rules! require_current {
            ($check:expr, $gate:literal) => {
                if !$check {
                    log::debug!(
                        "[tab-state/candidate] observation={} rejected gate={}",
                        envelope.observation_id,
                        $gate
                    );
                    #[cfg(test)]
                    eprintln!(
                        "[tab-state/candidate] observation={} rejected gate={}",
                        envelope.observation_id, $gate
                    );
                    return false;
                }
            };
        }
        require_current!(self.host.connection_is_current(adapter_id), "connection");
        require_current!(
            self.host
                .supporter_models()
                .get(adapter_id)
                .is_some_and(|current| Arc::ptr_eq(&current, adapter)),
            "adapter_identity"
        );
        require_current!(
            coordinator::global().is_current(&envelope.observation),
            "coordinator"
        );
        require_current!(
            self.host.ledger_snapshot(&self.tab) == envelope.ledgers,
            "ledgers"
        );
        require_current!(
            self.host.observation_is_current(&self.tab, &self.metadata),
            "observation_identity"
        );
        true
    }

    fn record(&mut self, source: identity::DecisionSource, verdict: Option<&CandidateVerdict>) {
        if let Some(envelope) = self.metadata.envelope.as_ref() {
            if source != identity::DecisionSource::Adapter || verdict.is_none() {
                safe_response::cancel_for_correlation(&self.tab, &envelope.correlation);
            }
            observer::record(
                &self.host,
                &self.tab,
                identity::correlation::RecordIdentity::captured(
                    envelope.observation_id,
                    &envelope.correlation,
                )
                .with_model_verdict(self.model_verdict.clone()),
                source,
                verdict.map_or(OTHER, |verdict| verdict.label.as_str()),
                verdict.map(|verdict| verdict.confidence),
                &self.metadata.title,
            );
        }
        self.recorded = true;
    }
}

impl<H: ShadowHost> Drop for Pending<H> {
    fn drop(&mut self) {
        if !self.recorded {
            self.record(self.source, None);
        }
    }
}

#[cfg(test)]
pub(crate) fn spawn<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    tab: &str,
    expected_screen: Option<&str>,
    decided: impl FnOnce(Option<CandidateVerdict>, identity::DecisionSource, &dyn Fn() -> bool)
        + Send
        + 'static,
) -> Option<scheduler::ObservationHandle> {
    spawn_with_origin(
        host,
        config,
        tab,
        expected_screen,
        ObservationTrigger::OutputCandidate,
        None,
        None,
        decided,
    )
}

#[cfg(test)]
pub(crate) fn spawn_with_origin<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    tab: &str,
    expected_screen: Option<&str>,
    trigger: ObservationTrigger,
    parent_observation_id: Option<u64>,
    expected_local: Option<&identity::LocalIdentity>,
    decided: impl FnOnce(Option<CandidateVerdict>, identity::DecisionSource, &dyn Fn() -> bool)
        + Send
        + 'static,
) -> Option<scheduler::ObservationHandle> {
    spawn_with_origin_and_id(
        host,
        config,
        tab,
        expected_screen,
        trigger,
        parent_observation_id,
        expected_local,
        decided,
    )
    .map(|(handle, _)| handle)
}

pub(crate) fn spawn_with_origin_and_id<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    tab: &str,
    expected_screen: Option<&str>,
    trigger: ObservationTrigger,
    parent_observation_id: Option<u64>,
    expected_local: Option<&identity::LocalIdentity>,
    decided: impl FnOnce(Option<CandidateVerdict>, identity::DecisionSource, &dyn Fn() -> bool)
        + Send
        + 'static,
) -> Option<(scheduler::ObservationHandle, u64)> {
    let adapter_id = config.active_id()?.to_owned();
    if !observer::connected(host, config, &adapter_id) {
        log::debug!("[tab-state/candidate] capture rejected: connection");
        #[cfg(test)]
        eprintln!("[tab-state/candidate] capture rejected: connection");
        return None;
    }
    let adapter = supporter::configured_model(config, &host.supporter_models())?;
    let (mut request, mut metadata) = observer::prepare_observation_with_parent_and_identity(
        host,
        tab,
        trigger,
        "scrape_candidate",
        parent_observation_id,
        expected_local,
    )
    .inspect_err(|reason| {
        log::debug!("[tab-state/candidate] capture rejected: {reason}");
        #[cfg(test)]
        eprintln!("[tab-state/candidate] capture rejected: {reason}");
    })
    .ok()?;
    let envelope = metadata.envelope.as_mut()?;
    let observation_id = envelope.observation_id;
    envelope.adapter = Some(adapter.clone());
    envelope.policy_hash = Some(identity::supporter_policy(config));
    let mut pending = Pending {
        host: host.clone(),
        tab: tab.into(),
        metadata,
        source: identity::DecisionSource::unavailable(),
        recorded: false,
        model_verdict: None,
    };
    if expected_screen.is_some_and(|screen| {
        pending
            .metadata
            .envelope
            .as_ref()
            .is_none_or(|envelope| frame_hash(&screen_tail(screen)) != envelope.raw_tail_hash)
    }) {
        log::debug!(
            "[tab-state/candidate] observation={observation_id} rejected gate=expected_frame"
        );
        #[cfg(test)]
        eprintln!(
            "[tab-state/candidate] observation={observation_id} rejected gate=expected_frame"
        );
        return None;
    }
    request.questions.retain(|key, _| {
        matches!(
            key.as_str(),
            "label" | "prompt_visible" | "input_area" | "safe_choice"
        )
    });
    let criteria_version = supporter::criteria_hash(&serde_json::to_vec(&request.questions).ok()?);
    let config = config.clone();
    pending.source = identity::DecisionSource::Budget;
    let critical = matches!(
        trigger,
        ObservationTrigger::PromptEstablished
            | ObservationTrigger::PromptConfirmation
            | ObservationTrigger::SentinelRequest
    );
    scheduler::global()
        .submit(tab.into(), critical, request.questions.len(), move || {
            pending.source = identity::DecisionSource::unavailable();
            if !pending.current(&adapter_id, &adapter) {
                return None;
            }
            let captured_at = pending.metadata.envelope.as_ref()?.captured_at;
            let runtime = pending.host.supporter_runtime();
            let receipt_deadline = receipt_authorization_deadline(captured_at, config.deadline_ms);
            let adapter_deadline =
                runtime.decision_deadline(&adapter_id, config.deadline_ms, Instant::now());
            let result = if observer::choice_model(&pending.host.supporter_models(), &adapter_id)
                .is_none()
            {
                Err(DecisionError::Unsupported)
            } else {
                let model = supporter::budgeted_model_with_activity(
                    adapter.clone(),
                    runtime,
                    config.calls_per_hour.unwrap_or(0),
                    VerdictKind::TabState,
                    Some(pending.tab.clone()),
                    Some(criteria_version),
                    Some(supporter::ActivityContext::TabState {
                        title: pending.metadata.title.clone(),
                        declared_safe_choice: pending.metadata.declared_safe_choice.clone(),
                        decision_point: trigger.label().into(),
                    }),
                );
                model.decide_questions(&request, adapter_deadline)
            };
            pending.model_verdict = result
                .as_ref()
                .ok()
                .and_then(identity::correlation::ModelVerdict::from_response);
            if !pending.current(&adapter_id, &adapter) {
                return None;
            }
            let (mut verdict, source) = select(result);
            if !request.questions.contains_key("safe_choice") {
                if let Some(verdict) = verdict.as_mut() {
                    verdict.safe_choice = None;
                }
            }
            pending.record(source, verdict.as_ref());
            let envelope = pending.metadata.envelope.as_ref()?;
            let ledgers = pending.host.ledger_snapshot(&pending.tab);
            let ticket = coordinator::global().claim(&envelope.observation, ledgers.clone())?;
            if !coordinator::global().ticket_is_current(&ticket, &ledgers)
                || !pending.current(&adapter_id, &adapter)
            {
                return None;
            }
            if let Some(verdict) = verdict.as_mut() {
                verdict.receipt = Some(safe_response::Receipt {
                    metadata: pending.metadata.clone(),
                    adapter_id: adapter_id.clone(),
                    ticket: ticket.clone(),
                    deadline: receipt_deadline,
                });
            }
            let valid = || {
                coordinator::global()
                    .ticket_is_current(&ticket, &pending.host.ledger_snapshot(&pending.tab))
                    && pending.current(&adapter_id, &adapter)
            };
            decided(verdict, source, &valid);
            None
        })
        .map(|handle| (handle, observation_id))
}

impl ServiceContext {
    pub(crate) fn scrape_model_connected(&self) -> bool {
        let config = self.supporter_decision_config();
        config
            .active_id()
            .is_some_and(|adapter| observer::connected(self, &config, adapter))
    }

    pub(crate) fn observe_scrape_candidate(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        fallback: impl FnOnce(&ServiceContext) + Send + 'static,
    ) {
        let _ = self.observe_scrape_candidate_from(
            snapshot,
            fallback,
            ObservationTrigger::ParkedEdge,
            None,
            None,
        );
    }

    pub(crate) fn observe_scrape_candidate_from(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        fallback: impl FnOnce(&ServiceContext) + Send + 'static,
        trigger: ObservationTrigger,
        parent_observation_id: Option<u64>,
        expected_local: Option<&identity::LocalIdentity>,
    ) -> bool {
        self.observe_scrape_candidate_from_inner(
            snapshot,
            fallback,
            trigger,
            parent_observation_id,
            expected_local,
            None,
        )
        .is_some()
    }

    pub(super) fn observe_sentinel_prompt_candidate(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        expected_local: &identity::LocalIdentity,
        kind: safe_response::Kind,
        requested_by: &str,
        parent_observation_id: Option<u64>,
    ) -> Option<u64> {
        self.observe_scrape_candidate_from_inner(
            snapshot,
            |_| {},
            ObservationTrigger::SentinelRequest,
            parent_observation_id,
            Some(expected_local),
            Some((kind, requested_by.to_owned())),
        )
    }

    fn observe_scrape_candidate_from_inner(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        fallback: impl FnOnce(&ServiceContext) + Send + 'static,
        trigger: ObservationTrigger,
        parent_observation_id: Option<u64>,
        expected_local: Option<&identity::LocalIdentity>,
        sentinel_request: Option<(safe_response::Kind, String)>,
    ) -> Option<u64> {
        if sentinel_request
            .as_ref()
            .is_none_or(|(kind, _)| *kind != safe_response::Kind::Permission)
            && self
                .prompt_detector
                .lock()
                .active_prompt_explicit(&snapshot.tab_id)
                == Some(true)
        {
            return None;
        }
        let local = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&snapshot.tab_id) else {
                return None;
            };
            if tab.agent_type.as_deref() != Some(snapshot.agent_type.as_str())
                || tab.agent_session_id != snapshot.session_id
                || tab.cwd != snapshot.cwd
            {
                return None;
            }
            let local = identity::LocalIdentity::of_tab(tab);
            if expected_local.is_some_and(|expected| &local != expected) {
                return None;
            }
            local
        };
        let host = self.clone();
        let config = self.supporter_decision_config();
        let tab = snapshot.tab_id.clone();
        let screen = snapshot.screen_text.clone();
        let expected_local = local.clone();
        candidates::spawn_with_origin_and_id(
            self,
            &config,
            &tab,
            Some(&screen),
            trigger,
            parent_observation_id,
            Some(&expected_local),
            move |verdict, source, valid| {
                let correlation = verdict
                    .as_ref()
                    .and_then(|verdict| verdict.receipt.as_ref())
                    .and_then(|receipt| receipt.metadata.envelope.as_ref())
                    .map(|envelope| envelope.correlation.clone());
                if !host.observation_identity_is_current(&snapshot.tab_id, &local) {
                    if let Some(correlation) = correlation.as_ref() {
                        safe_response::cancel_for_correlation(&snapshot.tab_id, correlation);
                    }
                    return;
                }
                if source == identity::DecisionSource::Adapter {
                    if let Some(verdict) = verdict {
                        if let Some((kind, requested_by)) = sentinel_request.as_ref() {
                            if valid() {
                                host.try_sentinel_safe_response(
                                    &snapshot,
                                    &verdict,
                                    *kind,
                                    requested_by,
                                );
                            } else {
                                safe_response::cancel_for_verdict(&snapshot.tab_id, &verdict);
                            }
                        } else if trigger.is_internal_candidate() {
                            if valid() {
                                host.try_model_safe_response_from_receipt(
                                    &snapshot.tab_id,
                                    &verdict,
                                );
                            } else {
                                safe_response::cancel_for_verdict(&snapshot.tab_id, &verdict);
                            }
                        } else {
                            host.publish_model_scrape_candidate(&snapshot, &verdict, valid);
                        }
                    }
                } else {
                    if trigger.is_internal_candidate() {
                        if let Some(correlation) = correlation.as_ref() {
                            safe_response::cancel_for_correlation(&snapshot.tab_id, correlation);
                        }
                    } else {
                        fallback(&host);
                    }
                }
            },
        )
        .map(|(_, observation_id)| observation_id)
    }

    pub(crate) fn observe_model_prompt_established(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        parent_observation_id: u64,
    ) -> bool {
        let config = self.supporter_decision_config();
        let Some(adapter) = config.active_id() else {
            return false;
        };
        if !observer::connected(self, &config, adapter) {
            return false;
        }
        let Some(scope) = self.followup_scope(&snapshot.tab_id, &snapshot.screen_text) else {
            return false;
        };
        if !coordinator::global().claim_followup(&snapshot.tab_id, &scope, false) {
            return false;
        }
        self.observe_scrape_candidate_from(
            snapshot,
            |_| {},
            ObservationTrigger::PromptEstablished,
            Some(parent_observation_id),
            None,
        )
    }
}
