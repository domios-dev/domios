use super::*;
use codeterm_types::decision::DecisionError;

#[derive(Clone)]
pub(crate) struct EnvelopeContext {
    pub(crate) local: Option<identity::LocalIdentity>,
    pub(crate) correlation: identity::correlation::Correlation,
    pub(crate) frame_hash: u64,
    pub(super) captured_screen: String,
    pub(super) raw_tail_hash: u64,
    pub(super) observation: coordinator::ObservationIdentity,
    pub(super) ledgers: coordinator::LedgerSnapshot,
    pub(super) captured_at: Instant,
    pub(super) observation_id: u64,
    pub(crate) adapter: Option<Arc<dyn DecisionModel>>,
    pub(crate) policy_hash: Option<String>,
    pub(super) pty_session_generation: Option<u64>,
}

impl std::fmt::Debug for EnvelopeContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvelopeContext")
            .field("observation_id", &self.observation_id)
            .field("frame_hash", &self.frame_hash)
            .finish_non_exhaustive()
    }
}

struct QueuedObservation<H: ShadowHost> {
    host: H,
    tab: String,
    title: String,
    observation_id: u64,
    decision_point: &'static str,
    correlation: Option<identity::correlation::Correlation>,
    pending: bool,
    source: identity::DecisionSource,
}

impl<H: ShadowHost> Drop for QueuedObservation<H> {
    fn drop(&mut self) {
        if self.pending {
            record(
                &self.host,
                &self.tab,
                identity::correlation::RecordIdentity::with_correlation(
                    self.observation_id,
                    self.correlation.as_ref(),
                )
                .with_fallback_decision_point(self.decision_point),
                self.source,
                OTHER,
                None,
                &self.title,
            );
        }
    }
}

struct ObservationEnvelope {
    request: DecisionQuestionsRequest,
    metadata: TabStateMetadata,
    permit: state::ObservationPermit,
    adapter: Arc<dyn DecisionModel>,
    configured_deadline_ms: u64,
    audit_only: bool,
}

pub(crate) fn spawn_observer<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Option<scheduler::ObservationHandle> {
    if !connected(host, config, adapter_id) {
        return None;
    }
    let adapter = host.supporter_models().get(adapter_id)?;
    let mut prepared = prepare_observation(host, tab_id, trigger, structural_verdict).ok()?;
    prepared.1.envelope.as_mut()?.adapter = Some(adapter);
    enqueue_prepared(
        host,
        config,
        adapter_id,
        configured_deadline_ms,
        tab_id,
        trigger,
        structural_verdict,
        prepared,
    )
}

pub(crate) fn enqueue_prepared<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
    prepared: (DecisionQuestionsRequest, TabStateMetadata),
) -> Option<scheduler::ObservationHandle> {
    enqueue_prepared_with_completion(
        host,
        config,
        adapter_id,
        configured_deadline_ms,
        tab_id,
        trigger,
        structural_verdict,
        prepared,
        None,
    )
}

fn enqueue_prepared_with_completion<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
    prepared: (DecisionQuestionsRequest, TabStateMetadata),
    completion: Option<scheduler::Completion>,
) -> Option<scheduler::ObservationHandle> {
    if !connected(host, config, adapter_id) {
        if let Some(envelope) = prepared.1.envelope.as_ref() {
            record(
                host,
                tab_id,
                identity::correlation::RecordIdentity::captured(
                    envelope.observation_id,
                    &envelope.correlation,
                ),
                identity::DecisionSource::unavailable(),
                OTHER,
                None,
                &prepared.1.title,
            );
            coordinator::global().cancel_observation(&envelope.observation);
        }
        return None;
    }
    let host = host.clone();
    let options = config.clone();
    let tab = tab_id.to_string();
    let adapter = adapter_id.to_string();
    let structural = structural_verdict.to_string();
    let questions = prepared.0.questions.len();
    let restore = host.restore_observation(tab_id);
    let queued = QueuedObservation {
        host: host.clone(),
        tab: tab.clone(),
        title: prepared.1.title.clone(),
        observation_id: prepared.1.envelope.as_ref()?.observation_id,
        decision_point: trigger.label(),
        correlation: Some(prepared.1.envelope.as_ref()?.correlation.clone()),
        pending: true,
        source: identity::DecisionSource::Budget,
    };
    scheduler::global().submit_with_completion(
        tab.clone(),
        matches!(trigger, ObservationTrigger::TabExit),
        questions,
        move || {
            let mut queued = queued;
            queued.source = identity::DecisionSource::unavailable();
            let _restore = restore;
            let result = observe_guarded_inner(
                &host,
                &options,
                &adapter,
                configured_deadline_ms,
                &tab,
                trigger,
                &structural,
                Some(prepared),
            );
            queued.pending = false;
            result
        },
        completion,
    )
}

pub(super) fn observe_user_delivery_readiness(
    host: &super::ServiceContext,
    tab_id: &str,
    expected_local: Option<&identity::LocalIdentity>,
) -> Result<super::UserDeliveryObservation, String> {
    let pty_session_generation = host.user_delivery_pty_session_generation(tab_id)?;
    let config = host.supporter_decision_config();
    let adapter_id = config
        .active_id()
        .filter(|adapter| !adapter.trim().is_empty())
        .ok_or_else(|| "supporter_adapter_unavailable".to_string())?
        .to_string();
    let prepared = prepare_observation_with_parent_and_identity(
        host,
        tab_id,
        ObservationTrigger::UserDeliveryReadiness,
        "user_delivery_pending_binding",
        None,
        expected_local,
    )?;
    if host.user_delivery_pty_session_generation(tab_id)? != pty_session_generation {
        return Err("supporter_pty_generation_changed_during_capture".into());
    }
    let adapter = host
        .supporter_models()
        .get(&adapter_id)
        .ok_or_else(|| "supporter_adapter_unavailable".to_string())?;
    let mut prepared = prepared;
    let envelope = prepared
        .1
        .envelope
        .as_mut()
        .ok_or("supporter_observation_invalid")?;
    envelope.adapter = Some(adapter.clone());
    envelope.pty_session_generation = Some(pty_session_generation);
    let metadata = prepared.1.clone();
    let callback_metadata = metadata.clone();
    let callback_tab = tab_id.to_string();
    let callback_adapter = adapter_id.clone();
    let callback_model = adapter;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let completion: scheduler::Completion = Box::new(move |verdict| {
        let observation_id = callback_metadata
            .envelope
            .as_ref()
            .map_or(0, |envelope| envelope.observation_id);
        let Some(verdict) = verdict else {
            let _ = sender.send(Err("supporter_observation_unavailable".to_string()));
            return;
        };
        let decided_by = identity::DecisionSource::verdict(&verdict);
        let _ = sender.send(Ok(super::UserDeliveryObservation {
            tab_id: callback_tab,
            adapter_id: callback_adapter,
            adapter: callback_model,
            observation_id,
            pty_session_generation,
            decided_by,
            metadata: callback_metadata,
            verdict,
        }));
    });
    let queued = enqueue_prepared_with_completion(
        host,
        &config,
        &adapter_id,
        config.deadline_ms,
        tab_id,
        ObservationTrigger::UserDeliveryReadiness,
        "user_delivery_pending_binding",
        prepared,
        Some(completion),
    );
    if queued.is_none() {
        return Err("supporter_observation_not_queued".into());
    }
    receiver
        .recv_timeout(super::DEADLINE + Duration::from_secs(1))
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => {
                "supporter_observation_deadline".to_string()
            }
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                "supporter_observation_cancelled".to_string()
            }
        })?
}

pub(super) fn connected<H: ShadowHost>(host: &H, config: &SupporterConfig, adapter: &str) -> bool {
    if config.active_id() != Some(adapter) || host.supporter_models().get(adapter).is_none() {
        return false;
    }
    supporter::supporter_connected(
        config,
        &host.supporter_models(),
        &TokenSnapshot {
            usable: host.token_available(adapter),
            acl_invalid: false,
        },
    )
}

#[cfg(test)]
pub(super) fn observe_guarded<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Option<ShadowVerdict> {
    observe_guarded_inner(
        host,
        config,
        adapter_id,
        configured_deadline_ms,
        tab_id,
        trigger,
        structural_verdict,
        None,
    )
}

fn observe_guarded_inner<H: ShadowHost>(
    host: &H,
    config: &SupporterConfig,
    adapter_id: &str,
    configured_deadline_ms: u64,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
    prepared: Option<(DecisionQuestionsRequest, TabStateMetadata)>,
) -> Option<ShadowVerdict> {
    if !connected(host, config, adapter_id) {
        if let Some(envelope) = prepared
            .as_ref()
            .and_then(|(_, metadata)| metadata.envelope.as_ref())
        {
            record(
                host,
                tab_id,
                identity::correlation::RecordIdentity::captured(
                    envelope.observation_id,
                    &envelope.correlation,
                ),
                identity::DecisionSource::unavailable(),
                OTHER,
                None,
                prepared
                    .as_ref()
                    .map_or(tab_id, |(_, metadata)| metadata.title.as_str()),
            );
            coordinator::global().cancel_observation(&envelope.observation);
        }
        return None;
    }
    let (request, mut metadata) =
        prepared.or_else(|| prepare_observation(host, tab_id, trigger, structural_verdict).ok())?;
    let captured_title = metadata.title.clone();
    metadata.envelope.as_mut()?.policy_hash = Some(identity::supporter_policy(config));
    let envelope = metadata.envelope.as_ref()?;
    if !matches!(trigger, ObservationTrigger::TabExit)
        && (!coordinator::global().is_current(&envelope.observation)
            || host.ledger_snapshot(tab_id) != envelope.ledgers
            || !host.observation_is_current(tab_id, &metadata))
    {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::captured(
                envelope.observation_id,
                &envelope.correlation,
            ),
            identity::DecisionSource::unavailable(),
            OTHER,
            None,
            &captured_title,
        );
        coordinator::global().cancel_observation(&envelope.observation);
        return None;
    }
    if envelope.adapter.as_ref().is_some_and(|expected| {
        host.supporter_models()
            .get(adapter_id)
            .is_none_or(|current| !Arc::ptr_eq(&current, expected))
    }) {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::captured(
                envelope.observation_id,
                &envelope.correlation,
            ),
            identity::DecisionSource::unavailable(),
            OTHER,
            None,
            &captured_title,
        );
        return None;
    }
    let admission = if matches!(trigger, ObservationTrigger::TabExit) {
        state::audit_permit(tab_id, &metadata.session_key, envelope.observation_id)
            .ok_or(state::PermitError::BusyAlreadyObserved)
    } else {
        claim_observation(tab_id, &metadata.session_key, trigger.event())
    };
    let mut permit = match admission {
        Ok(permit) => permit,
        Err(_) => {
            record(
                host,
                tab_id,
                identity::correlation::RecordIdentity::captured(
                    envelope.observation_id,
                    &envelope.correlation,
                ),
                if matches!(trigger, ObservationTrigger::TabExit) {
                    identity::DecisionSource::unavailable()
                } else {
                    identity::DecisionSource::Budget
                },
                OTHER,
                None,
                &captured_title,
            );
            coordinator::global().cancel_observation(&envelope.observation);
            return None;
        }
    };
    permit.observation_id = envelope.observation_id;
    let Some(adapter) = choice_model(&host.supporter_models(), adapter_id) else {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::captured(
                permit.observation_id,
                &envelope.correlation,
            ),
            identity::DecisionSource::unavailable(),
            OTHER,
            None,
            &captured_title,
        );
        coordinator::global().cancel_observation(&envelope.observation);
        return None;
    };
    if request.questions.is_empty() || request.questions.len() > scheduler::MAX_QUESTIONS {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::captured(
                permit.observation_id,
                &envelope.correlation,
            ),
            identity::DecisionSource::Budget,
            OTHER,
            None,
            &captured_title,
        );
        return None;
    }
    let envelope = ObservationEnvelope {
        request,
        metadata,
        permit,
        adapter,
        configured_deadline_ms,
        audit_only: matches!(trigger, ObservationTrigger::TabExit),
    };
    let model = supporter::budgeted_model_with_activity(
        envelope.adapter.clone(),
        host.supporter_runtime(),
        config.calls_per_hour.unwrap_or(0),
        VerdictKind::TabState,
        Some(tab_id.into()),
        Some(criteria_version()),
        Some(supporter::ActivityContext::TabState {
            title: envelope.metadata.title.clone(),
            declared_safe_choice: envelope.metadata.declared_safe_choice.clone(),
            decision_point: trigger.label().into(),
        }),
    );
    observe(host, &model, config, adapter_id, tab_id, trigger, envelope)
}

pub(super) fn record<'a, H: ShadowHost>(
    host: &H,
    tab: &str,
    observation: impl Into<identity::correlation::RecordIdentity<'a>>,
    source: identity::DecisionSource,
    label: &str,
    confidence: Option<f64>,
    title: &str,
) {
    let event = identity::journal(tab, observation, source, label, confidence, title);
    let _ = host.supporter_runtime().decision(event.clone());
    host.record(&format!("[tab-state/decision] {event}"));
}

fn current<H: ShadowHost>(
    host: &H,
    adapter_id: &str,
    tab: &str,
    envelope: &ObservationEnvelope,
) -> bool {
    let Some(identity) = envelope.metadata.envelope.as_ref() else {
        return false;
    };
    host.connection_is_current(adapter_id)
        && host
            .supporter_models()
            .get(adapter_id)
            .is_some_and(|adapter| Arc::ptr_eq(&adapter, &envelope.adapter))
        && if envelope.audit_only {
            state::permit_is_current(tab, &envelope.metadata.session_key, &envelope.permit)
                && identity
                    .local
                    .as_ref()
                    .zip(host.local_identity(tab))
                    .is_none_or(|(observed, current)| observed.launch_key == current.launch_key)
        } else {
            coordinator::global().is_current(&identity.observation)
                && host.ledger_snapshot(tab) == identity.ledgers
                && host.observation_is_current(tab, &envelope.metadata)
        }
}

fn observe<H: ShadowHost>(
    host: &H,
    model: &Arc<dyn DecisionModel>,
    _config: &SupporterConfig,
    adapter_id: &str,
    tab_id: &str,
    trigger: ObservationTrigger,
    envelope: ObservationEnvelope,
) -> Option<ShadowVerdict> {
    let title = envelope.metadata.title.clone();
    if !current(host, adapter_id, tab_id, &envelope) {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::from_metadata(
                envelope.permit.observation_id,
                &envelope.metadata,
            ),
            identity::DecisionSource::unavailable(),
            OTHER,
            None,
            &title,
        );
        coordinator::global().cancel_observation(&envelope.metadata.envelope.as_ref()?.observation);
        return None;
    }
    let classified_frame_hash = envelope.metadata.envelope.as_ref()?.raw_tail_hash;
    let deadline = host.supporter_runtime().decision_deadline(
        adapter_id,
        envelope.configured_deadline_ms,
        Instant::now(),
    );
    let response = model.decide_questions(&envelope.request, deadline);
    let model_verdict = response
        .as_ref()
        .ok()
        .and_then(identity::correlation::ModelVerdict::from_response);
    let result = response.and_then(|response| {
        observation_answers(response)
            .map_err(|_| DecisionError::Parse("invalid_observation".into()))
    });
    if !current(host, adapter_id, tab_id, &envelope) {
        record(
            host,
            tab_id,
            identity::correlation::RecordIdentity::from_metadata(
                envelope.permit.observation_id,
                &envelope.metadata,
            )
            .with_model_verdict(model_verdict.clone()),
            identity::DecisionSource::unavailable(),
            OTHER,
            None,
            &title,
        );
        coordinator::global().cancel_observation(&envelope.metadata.envelope.as_ref()?.observation);
        return None;
    }
    let metadata = envelope.metadata;
    let permit = envelope.permit;
    match result {
        Ok(mut verdict) => {
            verdict.classified_frame_hash = classified_frame_hash;
            let source = identity::DecisionSource::verdict(&verdict);
            record(
                host,
                tab_id,
                identity::correlation::RecordIdentity::from_metadata(
                    permit.observation_id,
                    &metadata,
                )
                .with_model_verdict(model_verdict.clone()),
                source,
                &verdict.label,
                Some(verdict.confidence),
                &title,
            );
            if trigger == ObservationTrigger::UserDeliveryReadiness {
                verdict.action = SignalAction::None;
                return Some(verdict);
            }
            if matches!(trigger, ObservationTrigger::TabExit)
                && metadata
                    .tab_snapshot
                    .as_ref()
                    .is_some_and(super::super::general_agent::is_general_agent)
            {
                host.record(&format!(
                    "[tab-state] exit observation retained for audit tab={tab_id}"
                ));
                return Some(verdict);
            }
            let spawn_healthy = trigger.is_spawn()
                && confidence_band(verdict.confidence) == ConfidenceBand::Strong
                && matches!(verdict.label.as_str(), READY_IDLE | WORKING_NORMALLY);
            if verdict.label == STILL_STARTING
                && !trigger.after_startup_deadline()
                && metadata
                    .context
                    .get("startup_deadline_elapsed")
                    .and_then(Value::as_bool)
                    != Some(true)
            {
                host.record(&format!(
                    "[tab-state] suppressed early still_starting tab={tab_id} frame_hash={:016x}",
                    verdict.classified_frame_hash,
                ));
                return Some(verdict);
            }
            verdict.action = if spawn_healthy {
                if !permit_is_current_for_launch(tab_id, &metadata.session_key, &permit) {
                    host.record(&format!(
                        "[tab-state] stale observation discarded tab={tab_id} episode={} frame_hash={:016x}",
                        permit.episode,
                        verdict.classified_frame_hash,
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
                ) else {
                    host.record(&format!(
                        "[tab-state] stale observation discarded tab={tab_id} episode={} frame_hash={:016x}",
                        permit.episode,
                        verdict.classified_frame_hash,
                    ));
                    return None;
                };
                action
            };
            let ticket = if verdict.action == SignalAction::StrongAction {
                let identity = metadata.envelope.as_ref()?;
                let ticket = coordinator::global()
                    .claim(&identity.observation, host.ledger_snapshot(tab_id))?;
                if !coordinator::global().ticket_is_current(&ticket, &host.ledger_snapshot(tab_id))
                    || !host.observation_is_current(tab_id, &metadata)
                    || !host.connection_is_current(adapter_id)
                {
                    return None;
                }
                Some(ticket)
            } else {
                None
            };
            let recovery_handled = host.recover(
                tab_id,
                &metadata,
                permit.episode,
                permit.observation_id,
                trigger,
                &verdict,
                ticket.as_ref(),
            );
            if verdict.label == OTHER {
                return Some(verdict);
            }
            if spawn_healthy {
                host.supporter_runtime().action(ActionRecord {
                    kind: VerdictKind::TabState,
                    tab: Some(tab_id),
                    label: &verdict.label,
                    confidence: verdict.confidence,
                    action: Action::SpawnOk,
                    reason: trigger.label(),
                    asked: Some(supporter::tab_questions(&metadata.title, &verdict.label)),
                });
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
            if verdict.action != SignalAction::None && !spawn_healthy && !recovery_handled {
                host.supporter_runtime().action(ActionRecord {
                    kind: VerdictKind::TabState,
                    tab: Some(tab_id),
                    label: &verdict.label,
                    confidence: verdict.confidence,
                    action: if verdict.action == SignalAction::StrongAction {
                        Action::Strong
                    } else {
                        Action::WeakHint
                    },
                    reason: trigger.label(),
                    asked: Some(supporter::tab_questions(&metadata.title, &verdict.label)),
                });
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
                "[tab-state] tab={tab_id} event={} label={} confidence={:.3} episode={} frame_hash={:016x} observation_id={}",
                trigger.label(),
                verdict.label,
                verdict.confidence,
                permit.episode,
                verdict.classified_frame_hash,
                permit.observation_id,
            ));
            Some(verdict)
        }
        Err(error) => {
            record(
                host,
                tab_id,
                identity::correlation::RecordIdentity::from_metadata(
                    permit.observation_id,
                    &metadata,
                )
                .with_model_verdict(model_verdict.clone()),
                identity::DecisionSource::error(&error),
                OTHER,
                None,
                &title,
            );
            coordinator::global().cancel_observation(&metadata.envelope.as_ref()?.observation);
            None
        }
    }
}

pub(crate) fn prepare_observation<H: ShadowHost>(
    host: &H,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
) -> Result<(DecisionQuestionsRequest, TabStateMetadata), String> {
    prepare_observation_with_parent_and_identity(
        host,
        tab_id,
        trigger,
        structural_verdict,
        None,
        None,
    )
}

pub(crate) fn prepare_observation_with_parent_and_identity<H: ShadowHost>(
    host: &H,
    tab_id: &str,
    trigger: ObservationTrigger,
    structural_verdict: &str,
    parent_observation_id: Option<u64>,
    expected_local: Option<&identity::LocalIdentity>,
) -> Result<(DecisionQuestionsRequest, TabStateMetadata), String> {
    let mut capture = QueuedObservation {
        host: host.clone(),
        tab: tab_id.into(),
        title: tab_id.into(),
        observation_id: state::next_observation_id(),
        decision_point: trigger.label(),
        correlation: None,
        pending: true,
        source: identity::DecisionSource::unavailable(),
    };
    let captured_at = Instant::now();
    let local = host.local_identity(tab_id);
    if expected_local.is_some_and(|expected| local.as_ref() != Some(expected)) {
        return Err("stale expected capture identity".into());
    }
    let screen = {
        let transport = host.transport(tab_id)?;
        transport.read_screen(false)?
    };
    let text = screen
        .get("text")
        .and_then(Value::as_str)
        .ok_or("screen read returned no text")?;
    let mut metadata = host.metadata(tab_id, trigger, structural_verdict, text)?;
    capture.title = metadata.title.clone();
    if local != host.local_identity(tab_id) {
        return Err("stale capture".into());
    }
    let frame_hash = frame_hash(&screen.to_string());
    if parent_observation_id.is_none()
        && !matches!(
            trigger,
            ObservationTrigger::UserDeliveryReadiness
                | ObservationTrigger::PromptEstablished
                | ObservationTrigger::PromptConfirmation
                | ObservationTrigger::SentinelRequest
        )
        && !super::admission::claim(tab_id, &metadata.session_key, frame_hash, trigger)
    {
        capture.pending = false;
        return Err("unchanged frame or per-tab classification rate bound".into());
    }
    let ledgers = host.ledger_snapshot(tab_id);
    let prompt = metadata
        .tab_snapshot
        .as_ref()
        .and_then(identity::prompt_hash)
        .map(|prompt| super::frame_hash(&format!("{prompt}:{}", ledgers.prompt)));
    let observation =
        coordinator::global().observe(tab_id, &metadata.session_key, prompt, frame_hash);
    let correlation = identity::correlation::Correlation::capture(
        local.as_ref(),
        &metadata.session_key,
        trigger,
        trigger.producer(),
        parent_observation_id,
        observation.occurrence,
        capture.observation_id,
        frame_hash,
        metadata.context["safe_response_kind"].as_str(),
    );
    metadata.envelope = Some(EnvelopeContext {
        local,
        correlation,
        frame_hash,
        captured_screen: text.to_string(),
        raw_tail_hash: super::frame_hash(&screen_tail(text)),
        observation,
        ledgers,
        captured_at,
        observation_id: capture.observation_id,
        adapter: None,
        policy_hash: None,
        pty_session_generation: None,
    });
    metadata.context = privacy::value(&metadata.context);
    let safe_screen = privacy::screen(text);
    if !privacy::complete_screen(text) {
        metadata.declared_safe_choice = None;
    }
    if metadata
        .prompt_options
        .iter()
        .any(|option| privacy::text(&option.value) != option.value)
    {
        metadata.declared_safe_choice = None;
    }
    for option in &mut metadata.prompt_options {
        option.label = privacy::text(&option.label);
    }
    let request = DecisionQuestionsRequest {
        state: serde_json::json!({
            "screen": safe_screen,
            "context": metadata.context,
        }),
        questions: questions::observation_questions(
            &metadata,
            safe_choice::safe_choice_question(&metadata),
        ),
    };
    capture.pending = false;
    Ok((request, metadata))
}

pub(super) fn choice_model(
    models: &SupporterRegistry,
    adapter_id: &str,
) -> Option<Arc<dyn DecisionModel>> {
    models.get(adapter_id).filter(|model| {
        let p = model.primitives();
        p.choice && p.noul && model.supports_question_batch()
    })
}

pub(super) fn observation_answers(response: DecisionResponse) -> Result<ShadowVerdict, String> {
    let mut answers = response.answers;
    let label = answers
        .remove("label")
        .ok_or("decision model omitted label")?;
    let (label, confidence) = chosen_label(label)?;
    let Some(DecisionAnswer::Noul { p: prompt_visible }) = answers.remove("prompt_visible") else {
        return Err("decision model omitted prompt_visible".into());
    };
    if !prompt_visible.is_finite() || !(0.0..=1.0).contains(&prompt_visible) {
        return Err("decision model gave invalid prompt_visible".into());
    }
    let reported_prompt_visible = prompt_visible;
    let input_area = match answers.remove("input_area") {
        None => None,
        Some(DecisionAnswer::Choice {
            choice,
            probabilities,
            confidence,
        }) => {
            let confidence = confidence
                .or_else(|| probabilities.get(&choice).copied())
                .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                .ok_or("decision model gave invalid input_area confidence")?;
            if !matches!(
                choice.as_str(),
                "active_modal" | "composer" | "historical_or_draft_text" | OTHER
            ) {
                return Err("decision model gave unknown input_area".into());
            }
            Some((choice, confidence))
        }
        Some(_) => return Err("decision model gave invalid input_area".into()),
    };
    let prompt_visible = match input_area.as_ref() {
        Some((area, confidence)) if area == "active_modal" && *confidence >= 0.60 => {
            prompt_visible.min(*confidence)
        }
        Some(_) => 0.0,
        None => prompt_visible,
    };
    let safe_choice = answers.remove("safe_choice").and_then(|answer| {
        let DecisionAnswer::Choice {
            choice,
            probabilities,
            confidence,
        } = answer
        else {
            return None;
        };
        let confidence = confidence.or_else(|| probabilities.get(&choice).copied())?;
        (confidence.is_finite() && (0.0..=1.0).contains(&confidence))
            .then_some((choice, confidence))
    });
    Ok(ShadowVerdict {
        label,
        confidence,
        prompt_visible,
        reported_prompt_visible,
        input_area,
        safe_choice,
        action: SignalAction::None,
        classified_frame_hash: 0,
    })
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
