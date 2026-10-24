use super::{now_ms, LoginRequest};
use crate::service::{MuxEmitCause, ServiceContext};
use codeterm_plugin::provider::{AgentProvider, CliLoginCapability};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
type CodeSubmission = (String, oneshot::Sender<Result<(), String>>);

mod process;

struct Job {
    provider: String,
    nonce: Option<String>,
    state: Value,
    stop: Option<watch::Sender<bool>>,
    notification: Option<String>,
    input: Option<mpsc::Sender<CodeSubmission>>,
}

static ACTIVE: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
struct Permit(String);
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.lock().remove(&self.0);
    }
}

static START_GATE: Semaphore = Semaphore::const_new(1);

static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);

fn provider_for(
    svc: &ServiceContext,
    tab: &str,
    requested: Option<&str>,
) -> Result<Arc<dyn AgentProvider>, String> {
    let remembered = JOBS.lock().get(tab).map(|job| job.provider.clone());
    let requested = requested.or(remembered.as_deref());
    let id = {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or("Sign-in tab is unavailable")?;
        if let Some(requested) = requested {
            if target.description.as_deref()
                != Some(crate::service::general_agent::FIRST_RUN_TAB_MARKER)
                && target.agent_type.as_deref() != Some(requested)
            {
                return Err("Provider selection requires a Get started tab".into());
            }
            requested.to_owned()
        } else {
            target
                .agent_type
                .clone()
                .ok_or("Sign-in provider is unavailable")?
        }
    };
    svc.agent_registry
        .provider_arc(&id)
        .ok_or("Sign-in provider is unavailable".into())
}

pub(super) fn cleanup(svc: &ServiceContext) {
    JOBS.lock().retain(|tab, job| {
        svc.mux
            .read()
            .get_tab(tab)
            .is_some_and(|target| target.session_binding_nonce == job.nonce)
    });
}

pub(super) fn close(tab: &str) {
    JOBS.lock().remove(tab);
}

pub(super) fn has(tab: &str) -> bool {
    JOBS.lock().contains_key(tab)
}

pub(super) fn supported(svc: &ServiceContext, tab: &str, provider: Option<&str>) -> bool {
    provider_for(svc, tab, provider).is_ok_and(|provider| provider.cli_login().is_some())
}

fn state(provider: &dyn AgentProvider, cap: &CliLoginCapability, signed_in: bool) -> Value {
    json!({
        "state": if signed_in { "loaded" } else { "loginPending" }, "epoch": 0,
        "auth": if signed_in { "signedIn" } else { "signedOut" },
        "loginCodeInput": cap.code_input,
        "loginMethods": ["browser"], "loginProviderName": provider.display_name(),
        "loginChoices": cap.methods.iter().map(|method| json!({"id":method.id,"label":method.label})).collect::<Vec<_>>()
    })
}

async fn authenticated(provider: Arc<dyn AgentProvider>) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let probe = provider
            .auth_status_probe()
            .ok_or("Provider has no authentication status probe")?;
        let binary = provider
            .cli_binary()
            .ok_or("Provider has no authentication binary")?;
        let record = crate::service::provider_signals_probe::capture(binary, &probe)
            .ok_or("Authentication status could not be verified")?;
        provider
            .parse_auth_status(&record)
            .map(|auth| auth.logged_in)
            .ok_or("Provider returned an invalid authentication status")
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(str::to_string)
}

fn publish(svc: &ServiceContext, tab: &str) {
    let jobs = JOBS.lock();
    let Some(job) = jobs.get(tab) else {
        return;
    };
    let mut mux = svc.mux.write();
    let Some(target) = mux.get_tab_mut(tab).filter(|target| {
        target.session_binding_nonce == job.nonce
            && (target.tab_type == crate::multiplexer::types::TabType::Choice
                || target.agent_type.as_deref() == Some(job.provider.as_str()))
    }) else {
        return;
    };
    target.agent_runtime_state = Some(job.state.clone());
    let event = json!({"tab_id":tab,"session_binding_nonce":job.nonce,"state":job.state});
    drop(mux);
    drop(jobs);
    svc.ui.emit("agent-runtime-state", event);
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
}

pub(super) fn show(svc: &ServiceContext, tab: &str) -> Result<Value, String> {
    let jobs = JOBS.lock();
    let Some(job) = jobs.get(tab) else {
        drop(jobs);
        let provider = provider_for(svc, tab, None)?;
        let cap = provider.cli_login().ok_or("CLI sign-in unsupported")?;
        let mut snapshot = state(provider.as_ref(), &cap, false);
        snapshot["auth"] = json!("unknown");
        return Ok(json!({"login":null,"state":snapshot}));
    };
    if svc.mux.read().get_tab(tab).is_none_or(|target| {
        target.session_binding_nonce != job.nonce
            || (target.tab_type != crate::multiplexer::types::TabType::Choice
                && target.agent_type.as_deref() != Some(job.provider.as_str()))
    }) {
        return Err("Sign-in tab binding changed".into());
    }
    Ok(json!({"login":job.state.get("login"),"state":job.state}))
}

pub(super) async fn operate(
    svc: &ServiceContext,
    tab: &str,
    request: LoginRequest,
) -> Result<Value, String> {
    let provider = provider_for(svc, tab, request.provider_id.as_deref())?;
    let cap = provider.cli_login().ok_or("CLI sign-in unsupported")?;
    match request.action.as_deref() {
        Some("code") => {
            if !cap.code_input {
                return Err("Provider does not accept a sign-in code".into());
            }
            let code = request
                .code
                .filter(|code| {
                    !code.trim().is_empty() && code.len() <= 16384 && !code.contains(['\r', '\n'])
                })
                .ok_or("Invalid sign-in code")?;
            let input = JOBS
                .lock()
                .get(tab)
                .filter(|job| job.state["login"]["status"] == "pending")
                .and_then(|job| job.input.clone())
                .ok_or("No sign-in accepts a code")?;
            let (sent, receipt) = oneshot::channel();
            input
                .try_send((code, sent))
                .map_err(|_| "Sign-in code input is busy or unavailable")?;
            tokio::time::timeout(Duration::from_secs(30), receipt)
                .await
                .map_err(|_| "Sign-in code acknowledgement timed out")?
                .map_err(|_| "Sign-in code input closed")??;
            return show(svc, tab);
        }
        Some("cancel") => {
            let mut jobs = JOBS.lock();
            let job = jobs.get_mut(tab).ok_or("No pending CLI sign-in")?;
            if let Some(stop) = &job.stop {
                let _ = stop.send(true);
            }
            job.state["login"]["status"] = json!("canceled");
            if let Some(login) = job.state["login"].as_object_mut() {
                login.remove("url");
                login.remove("userCode");
            }
            drop(jobs);
            publish(svc, tab);
            return show(svc, tab);
        }
        Some("notify") => {
            let mut jobs = JOBS.lock();
            let job = jobs.get_mut(tab).ok_or("No pending CLI sign-in")?;
            if let Some(id) = &job.notification {
                return Ok(json!({"notificationId":id}));
            }
            let login = serde_json::from_value(job.state["login"].clone())
                .map_err(|_| "Invalid sign-in state")?;
            let body = super::relay::phone_body(&login)?;
            let id = svc.create_login_notification(tab, body)?;
            job.notification = Some(id.clone());
            return Ok(json!({"notificationId":id}));
        }
        None | Some("start" | "check") => {}
        _ => return Err("Unsupported CLI sign-in action".into()),
    }
    let _permit = tokio::time::timeout(Duration::from_secs(30), START_GATE.acquire())
        .await
        .map_err(|_| "Sign-in request timed out")?
        .map_err(|_| "Sign-in unavailable")?;
    if ACTIVE.lock().contains(provider.id()) {
        return Err(
            "A sign-in is already pending for this provider. Cancel it before trying again.".into(),
        );
    }
    let signed_in = authenticated(provider.clone()).await?;
    let nonce = svc
        .mux
        .read()
        .get_tab(tab)
        .ok_or("Sign-in tab is unavailable")?
        .session_binding_nonce
        .clone();
    let snapshot = state(provider.as_ref(), &cap, signed_in);
    JOBS.lock().insert(
        tab.into(),
        Job {
            provider: provider.id().into(),
            nonce,
            state: snapshot,
            stop: None,
            notification: None,
            input: None,
        },
    );
    publish(svc, tab);
    if signed_in || request.action.as_deref() == Some("check") {
        return show(svc, tab);
    }
    if !matches!(
        request.method,
        Some(codeterm_plugin::agent_runtime::RuntimeLoginMethod::Browser)
    ) {
        return Err("This provider supports browser sign-in".into());
    }
    let method = match request.account.as_deref() {
        Some(id) => cap.methods.iter().find(|method| method.id == id),
        None => cap.methods.first(),
    }
    .ok_or("Unsupported provider sign-in method")?;
    if !ACTIVE.lock().insert(provider.id().into()) {
        return Err("A provider sign-in is already pending".into());
    }
    let permit = Permit(provider.id().into());
    let (stop, stopping) = watch::channel(false);
    let (input, codes) = mpsc::channel(1);
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut jobs = JOBS.lock();
        let job = jobs.get_mut(tab).ok_or("Sign-in tab is unavailable")?;
        job.stop = Some(stop);
        job.input = Some(input);
        job.state["login"] = json!({"status":"pending","loginId":id,"method":"browser","expiresAtMs":now_ms().saturating_add(cap.timeout_ms.clamp(1000,900_000))});
    }
    publish(svc, tab);
    let owner = svc.clone();
    let target = tab.to_owned();
    let response = show(svc, tab)?;
    let args = method.args.clone();
    crate::async_runtime::spawn(async move {
        let _permit = permit;
        let result = process::run(
            &owner,
            &target,
            &id,
            process::Request {
                provider: provider.clone(),
                capability: cap,
                args,
                stop: stopping,
                codes,
            },
        )
        .await;
        let verified = match result {
            Ok(()) => authenticated(provider).await,
            Err(error) => Err(error),
        };
        finish(&owner, &target, &id, verified);
    });
    Ok(response)
}

fn finish(svc: &ServiceContext, tab: &str, id: &str, result: Result<bool, String>) {
    let mut jobs = JOBS.lock();
    let Some(job) = jobs
        .get_mut(tab)
        .filter(|job| job.state["login"]["loginId"] == id && job.stop.is_some())
    else {
        return;
    };
    job.stop = None;
    job.input = None;
    if job.state["login"]["status"] == "canceled" {
        return;
    }
    let success = matches!(result, Ok(true));
    job.state["state"] = json!(if success { "loaded" } else { "loginPending" });
    job.state["auth"] = json!(if success { "signedIn" } else { "signedOut" });
    job.state["login"] = if success {
        json!({"status":"completed","loginId":id})
    } else {
        json!({"status":"failed","loginId":id,"error":result.err().unwrap_or_else(|| "The provider is still signed out. Try again.".into())})
    };
    drop(jobs);
    publish(svc, tab);
}

fn observe(svc: &ServiceContext, tab: &str, id: &str, url: String) {
    let mut jobs = JOBS.lock();
    let Some(job) = jobs
        .get_mut(tab)
        .filter(|job| job.state["login"]["loginId"] == id && job.stop.is_some())
    else {
        return;
    };
    if job.state["login"]["url"] == url {
        return;
    }
    job.state["login"]["url"] = json!(url);
    drop(jobs);
    publish(svc, tab);
}

#[cfg(test)]
mod tests;
