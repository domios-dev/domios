use super::*;
use codeterm_types::decision::DecisionError;

#[derive(Debug, Clone)]
pub(crate) struct CandidateVerdict {
    pub(crate) label: String,
    pub(crate) confidence: f64,
    visible: bool,
    area: String,
}

impl CandidateVerdict {
    pub(crate) fn live_prompt(&self) -> bool {
        self.visible && self.area == "active_modal"
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
}

impl<H: ShadowHost> Pending<H> {
    fn current(&self, adapter_id: &str, adapter: &Arc<dyn DecisionModel>) -> bool {
        let Some(envelope) = self.metadata.envelope.as_ref() else {
            return false;
        };
        self.host.connection_is_current(adapter_id)
            && self
                .host
                .supporter_models()
                .get(adapter_id)
                .is_some_and(|current| Arc::ptr_eq(&current, adapter))
            && coordinator::global().is_current(&envelope.observation)
            && self.host.ledger_snapshot(&self.tab) == envelope.ledgers
            && self.host.observation_is_current(&self.tab, &self.metadata)
    }

    fn record(&mut self, source: identity::DecisionSource, verdict: Option<&CandidateVerdict>) {
        if let Some(envelope) = self.metadata.envelope.as_ref() {
            observer::record(
                &self.host,
                &self.tab,
                envelope.observation_id,
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

pub(crate) fn spawn<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    tab: &str,
    expected_screen: Option<&str>,
    decided: impl FnOnce(Option<CandidateVerdict>, identity::DecisionSource, &dyn Fn() -> bool)
        + Send
        + 'static,
) -> Option<scheduler::ObservationHandle> {
    let adapter_id = config.active_id()?.to_owned();
    if !observer::connected(host, config, &adapter_id) {
        return None;
    }
    let adapter = supporter::configured_model(config, &host.supporter_models())?;
    let (mut request, mut metadata) = prepare_observation(
        host,
        tab,
        ObservationTrigger::PromptCandidate,
        "scrape_candidate",
    )
    .ok()?;
    let envelope = metadata.envelope.as_mut()?;
    envelope.adapter = Some(adapter.clone());
    envelope.policy_hash = Some(identity::supporter_policy(config));
    let captured_at = envelope.captured_at;
    let mut pending = Pending {
        host: host.clone(),
        tab: tab.into(),
        metadata,
        source: identity::DecisionSource::Error,
        recorded: false,
    };
    if expected_screen.is_some_and(|screen| {
        pending
            .metadata
            .envelope
            .as_ref()
            .is_none_or(|envelope| frame_hash(&screen_tail(screen)) != envelope.raw_tail_hash)
    }) {
        return None;
    }
    request
        .questions
        .retain(|key, _| matches!(key.as_str(), "label" | "prompt_visible" | "input_area"));
    let criteria_version = supporter::criteria_hash(&serde_json::to_vec(&request.questions).ok()?);
    let config = config.clone();
    pending.source = identity::DecisionSource::Budget;
    scheduler::global().submit(tab.into(), false, request.questions.len(), move || {
        pending.source = identity::DecisionSource::Error;
        if !pending.current(&adapter_id, &adapter) {
            return None;
        }
        let deadline = captured_at + Duration::from_millis(config.deadline_ms.max(1)).min(DEADLINE);
        let result = if Instant::now() >= deadline {
            Err(DecisionError::Deadline)
        } else if observer::choice_model(&pending.host.supporter_models(), &adapter_id).is_none() {
            Err(DecisionError::Unsupported)
        } else {
            let model = supporter::budgeted_model_with_activity(
                adapter.clone(),
                pending.host.supporter_runtime(),
                config.calls_per_hour.unwrap_or(0),
                VerdictKind::TabState,
                Some(pending.tab.clone()),
                Some(criteria_version),
                Some(supporter::ActivityContext::TabState {
                    title: pending.metadata.title.clone(),
                    declared_safe_choice: None,
                }),
            );
            model.decide_questions(&request, deadline)
        };
        if !pending.current(&adapter_id, &adapter) {
            return None;
        }
        let (verdict, source) = select(result);
        pending.record(source, verdict.as_ref());
        let envelope = pending.metadata.envelope.as_ref()?;
        let ledgers = pending.host.ledger_snapshot(&pending.tab);
        let ticket = coordinator::global().claim(&envelope.observation, ledgers.clone())?;
        if !coordinator::global().ticket_is_current(&ticket, &ledgers)
            || !pending.current(&adapter_id, &adapter)
        {
            return None;
        }
        let valid = || {
            coordinator::global()
                .ticket_is_current(&ticket, &pending.host.ledger_snapshot(&pending.tab))
                && pending.current(&adapter_id, &adapter)
        };
        decided(verdict, source, &valid);
        None
    })
}

impl ServiceContext {
    pub(crate) fn scrape_model_connected(&self) -> bool {
        let config = crate::config::load_config().supporter;
        config
            .active_id()
            .is_some_and(|adapter| observer::connected(self, &config, adapter))
    }

    pub(crate) fn observe_scrape_candidate(
        &self,
        snapshot: crate::agent::tab_state::TabStateSnapshot,
        fallback: impl FnOnce(&ServiceContext) + Send + 'static,
    ) {
        if self
            .prompt_detector
            .lock()
            .active_prompt_explicit(&snapshot.tab_id)
            == Some(true)
        {
            return;
        }
        let local = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&snapshot.tab_id) else {
                return;
            };
            if tab.agent_type.as_deref() != Some(snapshot.agent_type.as_str())
                || tab.agent_session_id != snapshot.session_id
                || tab.cwd != snapshot.cwd
            {
                return;
            }
            identity::LocalIdentity::of_tab(tab)
        };
        let host = self.clone();
        let config = crate::config::load_config().supporter;
        let tab = snapshot.tab_id.clone();
        let screen = snapshot.screen_text.clone();
        let _ = spawn(
            self,
            &config,
            &tab,
            Some(&screen),
            move |verdict, source, valid| {
                if !host.observation_identity_is_current(&snapshot.tab_id, &local) {
                    return;
                }
                if let Some(verdict) =
                    verdict.filter(|_| source == identity::DecisionSource::Adapter)
                {
                    host.publish_model_scrape_candidate(&snapshot, &verdict, valid);
                } else {
                    fallback(&host);
                }
            },
        );
    }
}
