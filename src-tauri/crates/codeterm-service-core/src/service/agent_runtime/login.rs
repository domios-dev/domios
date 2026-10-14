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

#[path = "login/terminal.rs"]
mod terminal;

static OWNERS: LazyLock<Mutex<HashMap<String, Weak<Session>>>> = LazyLock::new(Default::default);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginRequest {
    #[serde(default)]
    pub action: Option<String>,
    pub method: Option<RuntimeLoginMethod>,
    #[serde(default)]
    pub api_key: Option<String>,
}
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LoginState {
    pub login: Option<RuntimeLoginState>,
}

pub fn forget(session: &Arc<Session>) {
    let mut owners = OWNERS.lock();
    if owners
        .get(&session.provider)
        .and_then(Weak::upgrade)
        .is_some_and(|owner| Arc::ptr_eq(&owner, session))
    {
        owners.remove(&session.provider);
    }
}
fn claim(session: &Arc<Session>) -> Result<(), String> {
    let mut owners = OWNERS.lock();
    if owners
        .get(&session.provider)
        .and_then(Weak::upgrade)
        .is_some()
    {
        return Err(
            "A sign-in is already pending for this provider. Cancel it before trying again.".into(),
        );
    }
    owners.insert(session.provider.clone(), Arc::downgrade(session));
    Ok(())
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
    let session = match engine::for_tab(tab) {
        Some(session) => session,
        None => {
            let _ = engine::ensure(svc, tab).await;
            engine::for_tab(tab).ok_or("Runtime binding unavailable")?
        }
    };
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|provider| provider.agent_runtime())
        .and_then(|runtime| runtime.login)
        .ok_or("Runtime sign-in is unsupported")?;
    if request.action.as_deref() == Some("terminal") {
        return terminal::launch(svc, tab, &session, capability.terminal_command).await;
    }
    let _permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| "Sign-in deadline exceeded")?
        .map_err(|_| "Runtime unavailable")?;
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
        claim(&session)?;
        session.login.lock().login = Some(RuntimeLoginState {
            status: RuntimeLoginStatus::Pending,
            method: Some(method.clone()),
            login_id: None,
            url: None,
            user_code: None,
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
    if result.is_err() {
        if cancel {
            return Err("Cancel failed. Try Cancel again.".into());
        }
        forget(&session);
        session.login.lock().login = Some(RuntimeLoginState {
            status: RuntimeLoginStatus::Failed,
            method: None,
            login_id: None,
            url: None,
            user_code: None,
            error: Some("Sign-in failed. Try again.".into()),
        });
        engine::publish(svc, tab, &session);
        return Err("Sign-in failed. Try again.".into());
    }
    let login = session.login.lock().login.clone();
    Ok(json!({"login":login}))
}

pub fn apply(svc: &ServiceContext, session: &Arc<Session>, epoch: u64, login: RuntimeLoginState) {
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
    tauri::async_runtime::spawn(task);
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
        return Err(RuntimeFailure::Refused(
            "Sign-in could not be verified".into(),
        ));
    }
    drop(permit);
    engine::ensure(svc, &context.tab_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_requires_exact_pending_login_and_canceled_cannot_resume() {
        let pending = RuntimeLoginState {
            status: RuntimeLoginStatus::Pending,
            login_id: Some("own".into()),
            method: Some(RuntimeLoginMethod::Browser),
            url: None,
            user_code: None,
            error: None,
        };
        let event = RuntimeLoginState {
            status: RuntimeLoginStatus::Completed,
            ..pending.clone()
        };
        assert!(matching_completion(Some(&pending), &event));
        assert!(!matching_completion(
            Some(&pending),
            &RuntimeLoginState {
                login_id: Some("foreign".into()),
                ..event.clone()
            }
        ));
        assert!(!matching_completion(
            Some(&RuntimeLoginState {
                status: RuntimeLoginStatus::Canceled,
                ..pending
            }),
            &event
        ));
        assert!(!matching_completion(None, &event));
    }
}
