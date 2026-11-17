use super::{
    engine::{self, Session},
    model::RuntimeFailure,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
    time::Duration,
};

mod account_context;
mod cli;
mod declared;
mod relay;
pub(crate) use account_context::OpenedContext;
pub(crate) use account_context::{close as close_account_context, open as open_account_context};

struct Completion(tokio::sync::watch::Sender<bool>);
impl Completion {
    fn complete(self) {
        self.0.send_replace(true);
    }
}
pub(crate) fn close_cli(tab: &str) {
    cli::close(tab);
}
#[path = "login/terminal.rs"]
mod terminal;

static OWNERS: LazyLock<Mutex<HashMap<(String, String), Weak<Session>>>> =
    LazyLock::new(Default::default);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginRequest {
    #[serde(default)]
    pub binding: Option<LoginBinding>,
    #[serde(default)]
    pub login_id: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub method_id: Option<String>,
    pub method: Option<RuntimeLoginMethod>,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginBinding {
    pub nonce: String,
    pub epoch: u64,
}
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LoginState {
    pub login: Option<RuntimeLoginState>,
    #[serde(skip)]
    pub phone_notification: Option<String>,
    #[serde(skip)]
    declared: Option<declared::Claim>,
}

fn validate_request(session: &Session, request: &LoginRequest) -> Result<(), String> {
    validate_login_binding(session, request.binding.as_ref())?;
    if let Some(id) = request.login_id.as_deref() {
        if session
            .login
            .lock()
            .login
            .as_ref()
            .and_then(|login| login.login_id.as_deref())
            != Some(id)
        {
            return Err("Sign-in challenge is no longer current".into());
        }
    }
    Ok(())
}

pub(super) fn validate_login_binding(
    session: &Session,
    binding: Option<&LoginBinding>,
) -> Result<(), String> {
    let context = session.context.lock();
    if binding.is_some_and(|binding| {
        binding.nonce != context.binding_nonce || binding.epoch != context.epoch
    }) {
        return Err("Sign-in belongs to a different runtime session".into());
    }
    Ok(())
}

pub(super) fn declared_operation_witness(operation: &RuntimeOperation) -> Option<(String, String)> {
    let RuntimeOperation::Control { action, value, .. } = operation else {
        return None;
    };
    if action != "login" {
        return None;
    }
    Some((
        value.get("methodId")?.as_str()?.into(),
        value.get("loginId")?.as_str()?.into(),
    ))
}

pub(super) fn validate_declared_witness(
    session: &Session,
    witness: &(String, String),
) -> Result<(), RuntimeFailure> {
    declared::validate_witness(session, witness)
}

pub fn forget(session: &Arc<Session>) {
    let key = (session.provider.clone(), session.account.lock().clone());
    let mut owners = OWNERS.lock();
    if owners
        .get(&key)
        .and_then(Weak::upgrade)
        .is_some_and(|owner| Arc::ptr_eq(&owner, session))
    {
        owners.remove(&key);
    }
}
/// One account sign-in may be pending across tabs; a finished or expired claim no longer blocks a new one.
fn claim_refusal(
    owner_tab: Option<&str>,
    owner_login: Option<&RuntimeLoginState>,
    tab: &str,
    now: u64,
) -> Option<String> {
    let owner_tab = owner_tab?;
    let live = owner_login.is_some_and(|login| {
        login.status == RuntimeLoginStatus::Pending
            && login.expires_at_ms.is_none_or(|expiry| expiry > now)
    });
    if !live {
        return None;
    }
    Some(if owner_tab == tab {
        "A sign-in is already pending in this tab. Finish or cancel it before trying again.".into()
    } else {
        format!("A sign-in for this provider is already pending in tab {owner_tab}. Finish or cancel it there before trying again.")
    })
}
fn claim(session: &Arc<Session>, tab: &str) -> Result<(), String> {
    let key = (session.provider.clone(), session.account.lock().clone());
    let mut owners = OWNERS.lock();
    if let Some(owner) = owners.get(&key).and_then(Weak::upgrade) {
        let owner_tab = owner.context.lock().tab_id.clone();
        let owner_login = owner.login.lock().login.clone();
        if let Some(refusal) = claim_refusal(Some(&owner_tab), owner_login.as_ref(), tab, now_ms())
        {
            return Err(refusal);
        }
    }
    owners.insert(key, Arc::downgrade(session));
    Ok(())
}
/// Login failures reach the card and the log; a submitted key never does, and provider text stays bounded.
fn safe_reason(reason: &str, secret: Option<&str>) -> String {
    let mut reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(secret) = secret.map(str::trim).filter(|secret| !secret.is_empty()) {
        reason = reason.replace(secret, "<redacted>");
    }
    if reason.chars().count() > 300 {
        reason = reason.chars().take(299).chain(['…']).collect();
    }
    if reason.is_empty() {
        "Sign-in failed. Try again.".into()
    } else {
        reason
    }
}
fn run_failure_reason(failure: &RuntimeFailure) -> String {
    let RuntimeFailure::Refused(reason) = failure else {
        return format!("Sign-in failed: {failure}");
    };
    let tag = format!("{:?}: ", RuntimeFailureCode::ProviderRefused);
    let reason = reason.strip_prefix(&tag).unwrap_or(reason);
    if reason.starts_with("Sign-in") {
        reason.to_owned()
    } else {
        format!("Sign-in failed: {reason}")
    }
}
fn request_label(request: &LoginRequest) -> String {
    let method = request
        .method_id
        .clone()
        .or_else(|| {
            request.method.as_ref().map(|method| {
                serde_json::to_value(method)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
        })
        .unwrap_or_default();
    format!(
        "{}{}{method}",
        request.action.as_deref().unwrap_or("start"),
        if method.is_empty() { "" } else { ":" }
    )
}
fn matching_completion(pending: Option<&RuntimeLoginState>, event: &RuntimeLoginState) -> bool {
    pending.is_some_and(|pending| {
        pending.status == RuntimeLoginStatus::Pending
            && match (&pending.login_id, &event.login_id) {
                (Some(expected), Some(actual)) => expected == actual,
                (None, None) => pending.method == Some(RuntimeLoginMethod::ApiKey),
                _ => false,
            }
    })
}

pub async fn operate(
    svc: &ServiceContext,
    tab: &str,
    request: LoginRequest,
) -> Result<Value, String> {
    let label = request_label(&request);
    let secret = request.api_key.clone().or_else(|| request.code.clone());
    cli::cleanup(svc);
    if !crate::service::provider_accounts::login_permitted(svc, tab) {
        return Err("This account sign-in is finishing; refresh its account card".into());
    }
    let owner = svc.clone();
    let target = tab.to_owned();
    let requested = request.provider_id.clone();
    let invocation = codeterm_plugin::js_runtime::spawn_blocking(move || {
        cli::invocation(&owner, &target, requested.as_deref())
    })
    .await
    .map_err(|error| error.to_string())??;
    codeterm_plugin::provider_account::scope(invocation, operate_unlogged(svc, tab, request))
        .await
        .map_err(|reason| {
            let reason = safe_reason(&reason, secret.as_deref());
            log::warn!("[agent_runtime.login] tab={tab} request={label} refused: {reason}");
            reason
        })
}

async fn operate_unlogged(
    svc: &ServiceContext,
    tab: &str,
    request: LoginRequest,
) -> Result<Value, String> {
    cli::cleanup(svc);
    if cli::supported(svc, tab, request.provider_id.as_deref()) {
        return cli::operate(svc, tab, request).await;
    }
    if request.provider_id.is_some() || request.account.is_some() {
        return Err("This provider does not support CLI account sign-in".into());
    }
    let session = match engine::for_tab(tab) {
        Some(session) => session,
        None => {
            let _ = engine::ensure(svc, tab).await;
            engine::for_tab(tab).ok_or("Runtime binding unavailable")?
        }
    };
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    validate_request(&session, &request)?;
    if declared::handles(&session, &request) {
        return declared::operate(svc, tab, &session, request).await;
    }
    if request.code.is_some() {
        return Err("This provider does not accept a browser sign-in code".into());
    }
    if let Some(method) = request.method_id.as_deref() {
        if request.action.is_some() || request.method.is_some() {
            return Err("Invalid declared sign-in request".into());
        }
        if let Some(key) = request.api_key.as_deref() {
            return super::auth_env::sign_in_with_key(svc, tab, &session, method, key).await;
        }
        if session.login.lock().declared.is_some() {
            claim(&session, tab)?;
            session.login.lock().declared = None;
            forget(&session);
        }
        return super::declared_controls::authenticate(
            svc,
            tab,
            &session,
            method,
            request.binding.as_ref(),
        )
        .await;
    }
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|provider| provider.agent_runtime())
        .and_then(|runtime| runtime.login);
    let Some(capability) = capability else {
        if request.method == Some(RuntimeLoginMethod::ApiKey)
            && request
                .action
                .as_deref()
                .is_none_or(|action| action == "start")
        {
            let methods = super::auth_env::selected_profile(svc, tab, &session)
                .map(|profile| super::auth_env::api_key_methods(&profile))
                .unwrap_or_default();
            let method = super::auth_env::default_api_key_method(&methods)?;
            let key = request.api_key.as_deref().unwrap_or_default();
            return super::auth_env::sign_in_with_key(svc, tab, &session, method, key).await;
        }
        return Err("Runtime sign-in is unsupported".into());
    };
    if request.action.as_deref() == Some("terminal") {
        return terminal::launch(svc, tab, &session, capability.terminal_command).await;
    }
    let _permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| "Sign-in deadline exceeded")?
        .map_err(|_| "Runtime unavailable")?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    validate_request(&session, &request)?;
    if request.action.as_deref() == Some("notify") {
        return relay::notify(svc, tab, &session);
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime unavailable")?;
    let context = session.context.lock().clone();
    let cancel = request.action.as_deref() == Some("cancel");
    let value = if cancel {
        let id = session
            .login
            .lock()
            .login
            .as_ref()
            .filter(|login| login.status == RuntimeLoginStatus::Pending)
            .and_then(|login| login.login_id.clone())
            .ok_or("No cancelable sign-in is pending")?;
        json!({"action":"cancel","loginId":id})
    } else {
        if request
            .action
            .as_deref()
            .is_some_and(|action| action != "start")
        {
            return Err("Invalid sign-in action".into());
        }
        let method = request.method.ok_or("Sign-in method required")?;
        if !capability.methods.contains(&method) {
            return Err("Sign-in method unsupported".into());
        }
        if method == RuntimeLoginMethod::ApiKey
            && request
                .api_key
                .as_ref()
                .is_none_or(|key| key.trim().is_empty() || key.len() > 16384)
        {
            return Err("API key required (maximum 16 KiB)".into());
        }
        claim(&session, tab)?;
        session.login.lock().declared = None;
        session.login.lock().phone_notification = None;
        session.login.lock().login = Some(RuntimeLoginState {
            instructions: None,
            code_input: None,
            status: RuntimeLoginStatus::Pending,
            method: Some(method.clone()),
            login_id: None,
            url: None,
            user_code: None,
            expires_at_ms: None,
            error: None,
        });
        engine::publish(svc, tab, &session);
        json!({"action":"start","method":method,"apiKey":request.api_key})
    };
    let result = engine::run(
        svc,
        &session.provider,
        &session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id,
            action: "login".into(),
            value,
        },
    )
    .await;
    if let Err(failure) = result {
        if cancel {
            return Err(format!("Cancel failed: {failure}. Try Cancel again."));
        }
        forget(&session);
        let reason = safe_reason(&run_failure_reason(&failure), request.api_key.as_deref());
        session.login.lock().login = Some(RuntimeLoginState {
            instructions: None,
            code_input: None,
            status: RuntimeLoginStatus::Failed,
            method: None,
            login_id: None,
            url: None,
            user_code: None,
            expires_at_ms: None,
            error: Some(reason.clone()),
        });
        engine::publish(svc, tab, &session);
        return Err(reason);
    }
    show(svc, tab)
}

pub fn apply(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    mut login: RuntimeLoginState,
) {
    if session.context.lock().epoch != epoch {
        return;
    }
    if !declared::accepts(session, &login) {
        return;
    }
    if login.status == RuntimeLoginStatus::Pending {
        let now = now_ms();
        let mut expiry = login
            .expires_at_ms
            .unwrap_or(u64::MAX)
            .min(now.saturating_add(15 * 60 * 1000));
        let state = session.login.lock();
        if state.declared.is_some() {
            expiry = expiry.min(
                state
                    .login
                    .as_ref()
                    .and_then(|login| login.expires_at_ms)
                    .unwrap_or(expiry),
            );
        }
        login.expires_at_ms = Some(expiry);
    } else {
        login.url = None;
        login.user_code = None;
        login.instructions = None;
        login.code_input = None;
        login.expires_at_ms = None;
    }
    if matches!(
        login.status,
        RuntimeLoginStatus::Pending | RuntimeLoginStatus::Canceled
    ) {
        if login.status == RuntimeLoginStatus::Canceled {
            if !matching_completion(session.login.lock().login.as_ref(), &login) {
                return;
            }
            forget(session);
        }
        session.login.lock().login = Some(login);
        let tab = session.context.lock().tab_id.clone();
        engine::publish(svc, &tab, session);
        declared::start_callback(svc, session);
        return;
    }
    let owner = svc.clone();
    let session = session.clone();
    let task: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
        Box::pin(async move {
            let result = finish(&owner, &session, epoch, login).await;
            if let Err(reason) = result {
                if session.context.lock().epoch == epoch {
                    session.state.lock().degrade(epoch, &reason);
                    let tab = session.context.lock().tab_id.clone();
                    engine::publish(&owner, &tab, &session);
                }
            }
        });
    crate::async_runtime::spawn(task);
}
async fn finish(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    login: RuntimeLoginState,
) -> Result<(), RuntimeFailure> {
    let permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|_| RuntimeFailure::Superseded)?;
    engine::validate_binding(svc, session)?;
    if session.context.lock().epoch != epoch
        || !matching_completion(session.login.lock().login.as_ref(), &login)
    {
        return Ok(());
    }
    let successful = login.status == RuntimeLoginStatus::Completed;
    session.login.lock().login = Some(login);
    forget(session);
    let context = session.context.lock().clone();
    engine::publish(svc, &context.tab_id, session);
    if !successful {
        return Ok(());
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    engine::run(
        svc,
        &session.provider,
        session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id,
            action: "auth".into(),
            value: Value::Null,
        },
    )
    .await?;
    if *session.auth.lock() != RuntimeAuthState::SignedIn {
        return Err(RuntimeFailure::SignInRequired);
    }
    drop(permit);
    if let Err(error) = svc
        .refresh_remote_task_environments(&session.provider)
        .await
    {
        log::warn!("[agent_runtime.login] {error}");
    }
    engine::ensure(svc, &context.tab_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests;

pub fn show(svc: &ServiceContext, tab: &str) -> Result<Value, String> {
    if cli::has(tab) || cli::supported(svc, tab, None) {
        return cli::show(svc, tab);
    }
    let session = engine::for_tab(tab).ok_or("Runtime binding unavailable")?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    let login = project(session.login.lock().login.as_ref(), now_ms());
    let authentication = session.maps.lock().authentication.clone();
    let api_key_methods = super::auth_env::selected_profile(svc, tab, &session)
        .map(|profile| super::auth_env::api_key_methods(&profile))
        .unwrap_or_default();
    let context = session.context.lock();
    Ok(
        json!({"login":login,"authentication":authentication,"apiKeyMethods":api_key_methods,
        "loginBinding":{"nonce":context.binding_nonce,"epoch":context.epoch}}),
    )
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn project(login: Option<&RuntimeLoginState>, now: u64) -> Option<RuntimeLoginState> {
    let mut login = login?.clone();
    if login.status != RuntimeLoginStatus::Pending
        || login.expires_at_ms.is_some_and(|expiry| expiry <= now)
    {
        login.url = None;
        login.user_code = None;
        login.instructions = None;
        login.code_input = None;
        if login.status == RuntimeLoginStatus::Pending {
            login.status = RuntimeLoginStatus::Failed;
            login.error = Some("Sign-in expired. Cancel it and try again.".into());
        }
    }
    Some(login)
}
