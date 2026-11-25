use super::{engine, model::RuntimeFailure, process::Process};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeOperation, RuntimeSettings};
use codeterm_plugin::model_catalog;
use codeterm_types::agent::chat_types::ChatMsg;
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) const SUBTYPE: &str = "model_unavailable";

#[derive(Deserialize)]
pub(super) struct RefusedModel {
    pub model: String,
}

#[derive(Clone)]
pub(super) struct Fallback {
    implicit: bool,
    used: Arc<AtomicBool>,
    failure: Arc<parking_lot::Mutex<Option<RuntimeFailure>>>,
}

impl Fallback {
    pub fn new(implicit: bool) -> Self {
        Self {
            implicit,
            used: Arc::new(AtomicBool::new(false)),
            failure: Default::default(),
        }
    }

    fn eligible(&self, before_acceptance: bool, fresh: bool) -> bool {
        self.implicit && before_acceptance && fresh && !self.used.load(Ordering::Acquire)
    }
}

tokio::task_local! {
    pub(super) static FALLBACK: Fallback;
}

pub(super) fn record_launch_failure(failure: &RuntimeFailure) {
    let _ = FALLBACK.try_with(|launch| *launch.failure.lock() = Some(failure.clone()));
}

pub(super) fn take_launch_failure() -> Option<RuntimeFailure> {
    FALLBACK
        .try_with(|launch| launch.failure.lock().take())
        .ok()
        .flatten()
}

pub(super) fn settings(operation: &mut RuntimeOperation) -> Option<&mut RuntimeSettings> {
    match operation {
        RuntimeOperation::OpenThread { settings, .. }
        | RuntimeOperation::ResumeThread { settings, .. }
        | RuntimeOperation::SubmitTurn { settings, .. } => Some(settings),
        _ => None,
    }
}

pub(super) async fn apply_remembered(
    svc: &ServiceContext,
    session: &engine::Session,
    implicit: bool,
) -> Result<(), RuntimeFailure> {
    let provider = session.provider.clone();
    let invocation = session.invocation.lock().clone();
    let version = session.state.lock().binary_version.clone();
    let Ok(models) = tokio::task::spawn_blocking(move || {
        codeterm_plugin::provider_account::with_context(invocation, || {
            model_catalog::unavailable_models_for_version(&provider, version.as_deref())
        })
    })
    .await
    else {
        return Ok(());
    };
    engine::validate_binding(svc, session)?;
    session.state.lock().unavailable_models = models.clone();
    let selected = session.context.lock().settings.model.clone();
    let model = match selected {
        Some(model) if models.contains(&model) => Some(model),
        None if implicit => models.first().cloned(),
        _ => None,
    };
    let Some(model) = model else { return Ok(()) };
    if implicit {
        if let Some(alternate) = next_listed(session, &model).await {
            engine::validate_binding(svc, session)?;
            replace_model(&mut session.context.lock().settings, alternate);
            return Ok(());
        }
    }
    Err(RuntimeFailure::ModelUnavailable {
        model,
        message: "The provider previously refused this model for the current account and CLI."
            .into(),
    })
}

pub(super) fn replace_model(
    settings: &mut RuntimeSettings,
    model: crate::agent::provider::ModelInfo,
) {
    settings.model = Some(model.id);
    settings.effort = settings
        .effort
        .take()
        .filter(|effort| {
            model
                .reasoning_efforts
                .as_ref()
                .is_some_and(|levels| levels.iter().any(|level| &level.id == effort))
        })
        .or(model.default_reasoning_effort);
}

pub(super) async fn remember(
    svc: &ServiceContext,
    session: &Arc<engine::Session>,
    process: &Process,
    model: &str,
) {
    let epoch = session.context.lock().epoch;
    let (Some(adapter), Some(binary), Some(version)) = (
        session.adapter.lock().clone(),
        process.binary_fingerprint.clone(),
        session.state.lock().binary_version.clone(),
    ) else {
        return;
    };
    if engine::credentials_changed(svc, session).await
        || engine::validate_binding(svc, session).is_err()
    {
        return;
    }
    let model = model.to_owned();
    let invocation = session.invocation.lock().clone();
    let result = tokio::task::spawn_blocking(move || {
        codeterm_plugin::provider_account::with_context(invocation, || {
            model_catalog::remember_model_refusal(adapter.as_ref(), &binary, &version, &model)?;
            Ok::<_, std::io::Error>(model_catalog::unavailable_models(adapter.id()))
        })
    })
    .await;
    match result {
        Ok(Ok(models))
            if engine::validate_binding(svc, session).is_ok()
                && session.context.lock().epoch == epoch =>
        {
            if session.state.lock().unavailable_models != models {
                svc.ui.emit(
                    "provider-accounts-changed",
                    serde_json::json!({"providerId":session.provider}),
                );
            }
            session.state.lock().unavailable_models = models;
            let tab = session.context.lock().tab_id.clone();
            engine::publish_controls(svc, &tab, session);
            svc.emit_state_changed(crate::service::MuxEmitCause::RuntimeSignal);
        }
        Ok(Ok(_)) => {}
        error => log::warn!("[model-catalogue] could not store model refusal: {error:?}"),
    }
}

pub(super) async fn choose(
    svc: &ServiceContext,
    session: &engine::Session,
    context: &RuntimeContext,
    operation: &RuntimeOperation,
    model: &str,
    before_acceptance: bool,
) -> Option<crate::agent::provider::ModelInfo> {
    let fresh = match operation {
        RuntimeOperation::OpenThread { .. } => context.thread_id.is_none(),
        RuntimeOperation::SubmitTurn {
            active_turn_id: None,
            send_id,
            ..
        } => svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
            tab.pending_delivery
                .as_ref()
                .and_then(|pending| pending.send_id())
                == Some(send_id)
                && tab.session_binding_nonce.as_deref() == Some(&context.binding_nonce)
        }),
        _ => false,
    };
    if !FALLBACK
        .try_with(|permit| permit.eligible(before_acceptance, fresh))
        .unwrap_or(false)
    {
        return None;
    }
    let candidate = next_listed(session, model).await?;
    FALLBACK
        .try_with(|permit| (!permit.used.swap(true, Ordering::AcqRel)).then_some(candidate))
        .ok()
        .flatten()
}

async fn next_listed(
    session: &engine::Session,
    refused: &str,
) -> Option<crate::agent::provider::ModelInfo> {
    let adapter = session.adapter.lock().clone()?;
    let invocation = session.invocation.lock().clone();
    codeterm_plugin::provider_account::scope(invocation, async {
        let provider = adapter.id().to_owned();
        if !model_catalog::has_live_snapshot(&provider) {
            model_catalog::refresh_models(adapter, false).await?;
        }
        let refused = refused.to_owned();
        codeterm_plugin::js_runtime::spawn_blocking(move || {
            model_catalog::resolve_live_models(&provider)?
                .into_iter()
                .find(|model| {
                    model.available != Some(false)
                        && model.id != refused
                        && model_catalog::resolve_endpoint_model(&provider, &model.id).is_none()
                })
        })
        .await
        .ok()
        .flatten()
    })
    .await
}

pub(super) fn publish(
    svc: &ServiceContext,
    session: &engine::Session,
    context: &RuntimeContext,
    subtype: &str,
    content: String,
    model: &str,
) {
    if engine::validate_binding(svc, session).is_err()
        || session.context.lock().epoch != context.epoch
    {
        return;
    }
    let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let item = session.cache.lock().reconcile_live(
        ChatMsg {
            id: format!("{subtype}:{}", uuid::Uuid::new_v4().simple()),
            kind: "system".into(),
            system_subtype: Some(subtype.into()),
            content,
            tool_input: Some(serde_json::json!({"model":model})),
            ..Default::default()
        },
        at,
    );
    crate::service::tab_chat::structured_sink::emit_chat_msgs(
        svc.ui.as_ref(),
        &context.tab_id,
        &[item],
        None,
        context.thread_id.as_deref(),
        Some(&context.binding_nonce),
    );
}

pub(super) fn persist_fallback(
    svc: &ServiceContext,
    session: &engine::Session,
    context: &RuntimeContext,
    settings: &RuntimeSettings,
) -> Result<(), RuntimeFailure> {
    engine::validate_binding(svc, session)?;
    let mut current = session.context.lock();
    if current.epoch != context.epoch || current.thread_id != context.thread_id {
        return Err(RuntimeFailure::Superseded);
    }
    current.settings.model = settings.model.clone();
    current.settings.effort = settings.effort.clone();
    let confirmed = current.settings.clone();
    drop(current);
    super::selections::persist(svc, session, context, settings, &confirmed)
}

#[cfg(test)]
mod tests;
