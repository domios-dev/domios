use super::engine::{self, Session};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult, RuntimeSettings};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

pub(super) fn project(session: &Session, snapshot: &mut Map<String, Value>) {
    let context = session.context.lock();
    snapshot.insert("controls".into(), json!(context.settings));
    drop(context);
    snapshot.insert("notice".into(), json!(session.state.lock().notice.clone()));
    let maps = session.maps.lock();
    if let Some(authentication) = &maps.authentication {
        snapshot.insert("authentication".into(), json!(authentication));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub id: String,
    pub value: String,
    #[serde(default)]
    pub mode: bool,
}

pub const RUNTIME_BUSY_REASON: &str = "runtime_busy";
pub const RUNTIME_BUSY_MESSAGE: &str = "The agent is busy. Try again in a moment.";

#[derive(Debug, PartialEq, Eq)]
pub enum ControlError {
    /// Another runtime operation held the gate for the whole wait; nothing was sent.
    Busy,
    Refused(String),
}

impl From<String> for ControlError {
    fn from(reason: String) -> Self {
        Self::Refused(reason)
    }
}

impl From<&str> for ControlError {
    fn from(reason: &str) -> Self {
        Self::Refused(reason.to_owned())
    }
}

pub async fn select(
    svc: &ServiceContext,
    tab: &str,
    request: Selection,
) -> Result<Value, ControlError> {
    let session = engine::for_tab(tab).ok_or("Runtime unavailable")?;
    let Ok(permit) = tokio::time::timeout(super::controls::GATE_WAIT, session.gate.acquire()).await
    else {
        return Err(ControlError::Busy);
    };
    let _permit = permit.map_err(|_| "Runtime unavailable")?;
    engine::validate_binding(svc, &session).map_err(|e| e.to_string())?;
    let context = session.context.lock().clone();
    if context.thread_id.as_ref() != Some(&request.thread_id)
        || context.binding_nonce != request.binding_nonce
        || context.epoch != request.epoch
    {
        return Err("Control selection belongs to a different runtime session".into());
    }
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.restorable_launch.execution_sandbox.is_some())
    {
        return Err("Session controls cannot override an immutable execution sandbox".into());
    }
    let allowed = if request.mode {
        context
            .settings
            .modes
            .as_ref()
            .is_some_and(|modes| modes.iter().any(|mode| mode.id == request.value))
    } else {
        context
            .settings
            .config_options
            .as_ref()
            .is_some_and(|options| {
                options.iter().any(|option| {
                    option.id == request.id
                        && option
                            .options
                            .iter()
                            .any(|value| value.value == request.value)
                })
            })
    };
    if !allowed {
        return Err("Control value was not advertised by this session".into());
    }
    let category = (!request.mode)
        .then(|| {
            context
                .settings
                .config_options
                .as_ref()
                .and_then(|options| options.iter().find(|option| option.id == request.id))
                .and_then(|option| option.category.clone())
        })
        .flatten();
    let requested = RuntimeSettings {
        model: (category.as_deref() == Some("model")).then(|| request.value.clone()),
        effort: (category.as_deref() == Some("thought_level")).then(|| request.value.clone()),
        ..Default::default()
    };
    session.controls_confirmation.lock().take();
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime unavailable")?;
    let result = engine::run(
        svc,
        &session.provider,
        &session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id.clone(),
            action: if request.mode { "mode" } else { "config" }.into(),
            value: if request.mode {
                json!(request.value)
            } else {
                json!({"id":request.id,"value":request.value})
            },
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    if !matches!(result, RuntimeResult::Done) {
        return Err("Runtime did not apply requested control".into());
    }
    if request.mode || category.as_deref() == Some("mode") {
        super::permission_policy::record_selected_mode(svc, &session, &request.value);
    }
    if requested.model.is_some() || requested.effort.is_some() {
        let confirmed = session
            .controls_confirmation
            .lock()
            .take()
            .ok_or("Runtime did not confirm requested selection")?;
        super::selections::persist(svc, &session, &context, &requested, &confirmed)
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({"ok":true}))
}

pub(super) async fn authenticate(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    method: &str,
) -> Result<Value, String> {
    let _permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| "Sign-in deadline exceeded")?
        .map_err(|_| "Runtime unavailable")?;
    engine::validate_binding(svc, session).map_err(|e| e.to_string())?;
    let allowed = session
        .maps
        .lock()
        .authentication
        .as_ref()
        .is_some_and(|auth| {
            auth.methods
                .iter()
                .any(|value| value.id == method && value.available)
        });
    if !allowed {
        return Err("Authentication method was not declared or is unavailable".into());
    }
    let context = session.context.lock().clone();
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime unavailable")?;
    engine::run(
        svc,
        &session.provider,
        session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id,
            action: "login".into(),
            value: json!({"methodId":method}),
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    if *session.auth.lock() == codeterm_plugin::agent_runtime::RuntimeAuthState::SignedIn {
        if session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        {
            if let Some(thread) = &session.context.lock().thread_id {
                session.state.lock().load(context.epoch, thread);
            }
        } else {
            session.state.lock().phase = super::model::RuntimePhase::Loading;
        }
        drop(_permit);
        engine::ensure(svc, tab).await.map_err(|e| e.to_string())?;
    }
    engine::publish(svc, tab, session);
    super::login::show(svc, tab)
}

pub(super) fn notice(svc: &ServiceContext, session: &Session, message: String) {
    let tab = session.context.lock().tab_id.clone();
    session.state.lock().notice = Some(message);
    engine::publish_controls(svc, &tab, session);
}

pub(super) fn require_authentication(svc: &ServiceContext, session: &Session) {
    if let Some(auth) = session.maps.lock().authentication.as_mut() {
        auth.state = codeterm_plugin::agent_runtime::RuntimeAuthenticationState::NeedsAuth;
    }
    *session.auth.lock() = codeterm_plugin::agent_runtime::RuntimeAuthState::SignedOut;
    let context = session.context.lock().clone();
    session
        .state
        .lock()
        .degrade(context.epoch, &super::model::RuntimeFailure::SignInRequired);
    engine::publish(svc, &context.tab_id, session);
}

pub(super) fn reset(session: &Session, epoch: u64) {
    let mut context = session.context.lock();
    context.settings.commands = None;
    context.settings.config_options = None;
    context.settings.modes = None;
    drop(context);
    session.maps.lock().reset(epoch);
}
