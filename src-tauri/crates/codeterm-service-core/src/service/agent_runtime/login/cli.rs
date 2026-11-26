use super::{
    build_login_binding, local_installation_id, now_ms, validate_binding_fields,
    CodeSubmissionRecord, CodeSubmissionStatus, Completion, LoginRequest,
};
use crate::service::{MuxEmitCause, ServiceContext};
use codeterm_plugin::provider::{AgentProvider, CliLoginCapability};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, LazyLock,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
use zeroize::Zeroizing;
type CodeSubmission = (
    Zeroizing<String>,
    oneshot::Sender<process::CodeWriteOutcome>,
);

mod process;

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

struct Job {
    installation_id: Option<String>,
    provider: String,
    account: String,
    session_id: Option<String>,
    nonce: Option<String>,
    epoch: u64,
    expires_at_ms: Option<u64>,
    code_submission: Option<CodeSubmissionRecord>,
    state: Value,
    stop: Option<watch::Sender<bool>>,
    notification: Option<String>,
    input: Option<mpsc::Sender<CodeSubmission>>,
}

struct Run {
    id: String,
    provider: String,
    account: String,
    nonce: Option<String>,
    stop: watch::Sender<bool>,
    finished: watch::Receiver<bool>,
}

static RUNS: LazyLock<Mutex<HashMap<String, Arc<Run>>>> = LazyLock::new(Default::default);

static ACTIVE: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Default::default);
struct Permit((String, String));
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

fn matches_tab(job: &Job, target: &crate::multiplexer::types::Tab) -> bool {
    target.session_binding_nonce == job.nonce
        && target.agent_session_id == job.session_id
        && target
            .restorable_launch
            .account_id
            .as_deref()
            .unwrap_or("default")
            == job.account
        && (target.tab_type == crate::multiplexer::types::TabType::Choice
            || target.agent_type.as_deref() == Some(job.provider.as_str()))
}

fn tab_identity(
    svc: &ServiceContext,
    tab: &str,
) -> Result<(String, Option<String>, Option<String>), String> {
    let mux = svc.mux.read();
    let target = mux.get_tab(tab).ok_or("Sign-in tab is unavailable")?;
    Ok((
        target
            .restorable_launch
            .account_id
            .clone()
            .unwrap_or_else(|| "default".into()),
        target.session_binding_nonce.clone(),
        target.agent_session_id.clone(),
    ))
}

fn selected(svc: &ServiceContext, tab: &str) -> Result<(String, Option<String>), String> {
    let (account, nonce, _) = tab_identity(svc, tab)?;
    Ok((account, nonce))
}

pub(super) fn invocation(
    svc: &ServiceContext,
    tab: &str,
    requested: Option<&str>,
) -> Result<Option<Arc<codeterm_plugin::provider_account::InvocationContext>>, String> {
    let provider = provider_for(svc, tab, requested)?;
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|target| target.agent_type.as_deref() == Some(provider.id()))
    {
        return crate::service::provider_accounts::for_tab(svc, tab);
    }
    let account = selected(svc, tab)?.0;
    crate::service::provider_accounts::invocation(svc, provider.id(), Some(&account))
}

pub(super) fn cleanup(svc: &ServiceContext) {
    JOBS.lock().retain(|tab, job| {
        let current = svc
            .mux
            .read()
            .get_tab(tab)
            .is_some_and(|target| matches_tab(job, target));
        if !current {
            if let Some(stop) = &job.stop {
                stop.send_replace(true);
            }
        }
        current
    });
}

pub(super) fn close(tab: &str) {
    if let Some(run) = RUNS.lock().get(tab) {
        run.stop.send_replace(true);
    }
    JOBS.lock().remove(tab);
}

pub(super) async fn close_and_wait(
    tab: &str,
    provider: &str,
    account: &str,
    nonce: Option<&str>,
) -> Result<(), String> {
    let run = RUNS.lock().get(tab).cloned();
    if let Some(run) = &run {
        if run.provider != provider || run.account != account || run.nonce.as_deref() != nonce {
            return Err("Sign-in process ownership changed; credentials were retained".into());
        }
        run.stop.send_replace(true);
    }
    let id = {
        let mut jobs = JOBS.lock();
        if let Some(job) = jobs.get_mut(tab) {
            if let Some(stop) = &job.stop {
                stop.send_replace(true);
            }
            job.input = None;
            let id = job.state["login"]["loginId"].as_str().map(str::to_owned);
            if let Some(login) = job.state.get_mut("login").and_then(Value::as_object_mut) {
                login.insert("status".into(), json!("canceled"));
                for field in ["url", "userCode", "instructions"] {
                    login.remove(field);
                }
            }
            id
        } else {
            run.as_ref().map(|run| run.id.clone())
        }
    };
    if let Some(run) = &run {
        let mut finished = run.finished.clone();
        tokio::time::timeout(Duration::from_secs(30), finished.wait_for(|done| *done))
            .await
            .map_err(|_| "Sign-in process cleanup timed out; account credentials were retained")?
            .map_err(|_| {
                "Sign-in process cleanup is unconfirmed; account credentials were retained"
            })?;
    }
    let mut jobs = JOBS.lock();
    if jobs
        .get(tab)
        .is_some_and(|job| job.state["login"]["loginId"].as_str() != id.as_deref())
    {
        return Err("Sign-in changed during cleanup; account credentials were retained".into());
    }
    jobs.remove(tab);
    drop(jobs);
    if let Some(run) = run {
        let mut runs = RUNS.lock();
        if runs.get(tab).is_some_and(|current| current.id == run.id) {
            runs.remove(tab);
        }
    }
    Ok(())
}

pub(super) fn has(tab: &str) -> bool {
    JOBS.lock().contains_key(tab)
}

pub(super) fn supported(svc: &ServiceContext, tab: &str, provider: Option<&str>) -> bool {
    provider_for(svc, tab, provider).is_ok_and(|provider| provider.cli_login().is_some())
}

fn state(
    provider: &dyn AgentProvider,
    cap: &CliLoginCapability,
    signed_in: bool,
    epoch: u64,
) -> Value {
    json!({
        "state": if signed_in { "loaded" } else { "loginPending" }, "epoch": epoch,
        "auth": if signed_in { "signedIn" } else { "signedOut" },
        "loginCodeInput": cap.code_input,
        "loginMethods": ["browser"], "loginProviderName": provider.display_name(),
        "loginChoices": cap.methods.iter().map(|method| json!({"id":method.id,"label":method.label})).collect::<Vec<_>>()
    })
}

fn job_binding(job: &Job, tab: &str) -> super::LoginBinding {
    super::LoginBinding {
        installation_id: job.installation_id.clone(),
        tab_id: Some(tab.to_owned()),
        provider_id: Some(job.provider.clone()),
        session_id: job.session_id.clone(),
        nonce: job.nonce.clone().unwrap_or_default(),
        epoch: job.epoch,
    }
}

fn project_job_state(job: &Job, now: u64) -> Value {
    let mut snapshot = job.state.clone();
    if job.state["login"]["status"] == "pending"
        && job.expires_at_ms.is_some_and(|expiry| expiry <= now)
    {
        if let Some(login) = snapshot.get_mut("login").and_then(Value::as_object_mut) {
            login.insert("status".into(), json!("failed"));
            login.insert(
                "error".into(),
                json!("Sign-in expired. Cancel it and try again."),
            );
            for field in ["url", "userCode", "instructions", "codeInput"] {
                login.remove(field);
            }
        }
    }
    snapshot
}

async fn authenticated(provider: Arc<dyn AgentProvider>) -> Result<bool, String> {
    codeterm_plugin::js_runtime::spawn_blocking(move || {
        let probe = provider
            .auth_status_probe()
            .ok_or("Provider has no authentication status probe")?;
        let binary = provider
            .cli_binary()
            .ok_or("Provider has no authentication binary")?;
        let install_dirs =
            provider.cli_install_dirs(crate::agent::provider::current_platform_tag());
        let record =
            crate::service::provider_signals_probe::capture_in(binary, &install_dirs, &probe)
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
    let Some(target) = mux
        .get_tab_mut(tab)
        .filter(|target| matches_tab(job, target))
    else {
        return;
    };
    target.agent_runtime_state = Some(job.state.clone());
    let event = json!({"tab_id":tab,"session_binding_nonce":job.nonce,"state":job.state});
    drop(mux);
    drop(jobs);
    svc.ui.emit("agent-runtime-state", event);
    crate::service::provider_accounts::login_changed(svc, tab);
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
}

pub(super) fn show(svc: &ServiceContext, tab: &str) -> Result<Value, String> {
    let jobs = JOBS.lock();
    let Some(job) = jobs.get(tab) else {
        drop(jobs);
        let provider = provider_for(svc, tab, None)?;
        let cap = provider.cli_login().ok_or("CLI sign-in unsupported")?;
        let (_, nonce, session_id) = tab_identity(svc, tab)?;
        let binding = build_login_binding(
            svc,
            tab,
            provider.id(),
            session_id.as_deref(),
            nonce.as_deref().unwrap_or(""),
            0,
        );
        let mut snapshot = state(provider.as_ref(), &cap, false, 0);
        snapshot["auth"] = json!("unknown");
        snapshot["loginBinding"] = serde_json::to_value(&binding).unwrap_or(Value::Null);
        return Ok(json!({"login":null,"state":snapshot,"loginBinding":binding}));
    };
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_none_or(|target| !matches_tab(job, target))
    {
        return Err("Sign-in tab binding changed".into());
    }
    let projected = project_job_state(job, now_ms());
    let binding = job_binding(job, tab);
    let mut payload = json!({
        "login": projected.get("login"),
        "state": projected,
        "loginBinding": binding,
    });
    if let Some(submission) = &job.code_submission {
        payload["codeSubmission"] = serde_json::to_value(submission).unwrap_or(Value::Null);
    }
    Ok(payload)
}

fn require_challenge(
    svc: &ServiceContext,
    tab: &str,
    job: &Job,
    request: &LoginRequest,
) -> Result<(), String> {
    if request
        .login_id
        .as_deref()
        .is_none_or(|id| job.state["login"]["loginId"].as_str() != Some(id))
        || svc
            .mux
            .read()
            .get_tab(tab)
            .is_none_or(|target| !matches_tab(job, target))
    {
        return Err("Sign-in challenge is no longer current".into());
    }
    Ok(())
}

pub(super) async fn operate(
    svc: &ServiceContext,
    tab: &str,
    mut request: LoginRequest,
) -> Result<Value, String> {
    let provider = provider_for(svc, tab, request.provider_id.as_deref())?;
    let cap = provider.cli_login().ok_or("CLI sign-in unsupported")?;
    let (account, nonce, session_id) = tab_identity(svc, tab)?;
    if request.action.as_deref() != Some("code") {
        if let Some(binding) = request.binding.as_ref() {
            let expected_epoch = JOBS.lock().get(tab).map(|job| job.epoch).unwrap_or(0);
            let expected = build_login_binding(
                svc,
                tab,
                provider.id(),
                session_id.as_deref(),
                nonce.as_deref().unwrap_or(""),
                expected_epoch,
            );
            validate_binding_fields(&expected, binding, false)?;
        }
    }
    match request.action.as_deref() {
        Some("code") => {
            if !cap.code_input {
                return Err("Provider does not accept a sign-in code".into());
            }
            if request.api_key.is_some() || request.method_id.is_some() || request.method.is_some()
            {
                return Err("Invalid sign-in code request".into());
            }
            let code = request
                .code
                .take()
                .filter(|code| {
                    !code.trim().is_empty() && code.len() <= 16384 && !code.contains(['\r', '\n'])
                })
                .ok_or("Invalid sign-in code")?;
            let binding = request
                .binding
                .as_ref()
                .ok_or("Sign-in code submission requires an active login binding")?;
            let (input, login_id) = {
                let mut jobs = JOBS.lock();
                let job = jobs.get_mut(tab).ok_or("No pending CLI sign-in")?;
                if request
                    .provider_id
                    .as_deref()
                    .is_some_and(|id| id != job.provider)
                {
                    return Err("Sign-in belongs to a different provider".into());
                }
                let expected = job_binding(job, tab);
                validate_binding_fields(&expected, binding, true)?;
                require_challenge(svc, tab, job, &request)?;
                if job.state["login"]["status"] != "pending" {
                    return Err("No sign-in accepts a code".into());
                }
                if job.expires_at_ms.is_some_and(|expiry| expiry <= now_ms()) {
                    job.input = None;
                    if let Some(login) = job.state.get_mut("login").and_then(Value::as_object_mut) {
                        login.insert("status".into(), json!("failed"));
                        login.insert(
                            "error".into(),
                            json!("Sign-in expired. Cancel it and try again."),
                        );
                        for field in ["url", "userCode", "instructions", "codeInput"] {
                            login.remove(field);
                        }
                    }
                    drop(jobs);
                    publish(svc, tab);
                    return Err("Sign-in expired. Cancel it and try again.".into());
                }
                match job.code_submission.as_ref().map(|record| &record.status) {
                    None => {}
                    Some(CodeSubmissionStatus::Uncertain) => {
                        return Err(
                            "Previous sign-in code delivery is uncertain and cannot be replayed. Cancel and start a new sign-in."
                                .into(),
                        );
                    }
                    Some(
                        CodeSubmissionStatus::Submitted
                        | CodeSubmissionStatus::Accepted
                        | CodeSubmissionStatus::Completed,
                    ) => {
                        return Err(
                            "A sign-in code was already submitted for this flow. Wait for completion or cancel and start a new sign-in."
                                .into(),
                        );
                    }
                    Some(CodeSubmissionStatus::Failed) => {
                        return Err(
                            "This sign-in flow failed. Cancel and start a new sign-in.".into()
                        );
                    }
                }
                let input = job.input.take().ok_or("No sign-in accepts a code")?;
                let login_id = job.state["login"]["loginId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let record = CodeSubmissionRecord {
                    status: CodeSubmissionStatus::Submitted,
                    login_id: login_id.clone(),
                    reason: None,
                };
                job.code_submission = Some(record.clone());
                job.state["codeSubmission"] = serde_json::to_value(&record).unwrap_or(Value::Null);
                (input, login_id)
            };
            publish(svc, tab);
            let (sent, receipt) = oneshot::channel();
            if input.try_send((code.into_zeroizing(), sent)).is_err() {
                let reason = "Sign-in code input is unavailable".to_owned();
                {
                    let mut jobs = JOBS.lock();
                    if let Some(job) = jobs
                        .get_mut(tab)
                        .filter(|job| job.state["login"]["loginId"].as_str() == Some(&login_id))
                    {
                        let record = CodeSubmissionRecord {
                            status: CodeSubmissionStatus::Failed,
                            login_id: login_id.clone(),
                            reason: Some(reason.clone()),
                        };
                        job.code_submission = Some(record.clone());
                        job.state["codeSubmission"] =
                            serde_json::to_value(&record).unwrap_or(Value::Null);
                    }
                }
                publish(svc, tab);
                return Err(reason);
            }
            let write_outcome = match tokio::time::timeout(Duration::from_secs(30), receipt).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => process::CodeWriteOutcome::Uncertain(
                    "Sign-in code acknowledgement channel closed; delivery is uncertain and was not replayed".into(),
                ),
                Err(_) => process::CodeWriteOutcome::Uncertain(
                    "Sign-in code acknowledgement timed out; delivery is uncertain and was not replayed".into(),
                ),
            };
            match write_outcome {
                process::CodeWriteOutcome::Written => {
                    let outcome = {
                        let mut jobs = JOBS.lock();
                        if let Some(job) = jobs
                            .get_mut(tab)
                            .filter(|job| job.state["login"]["loginId"].as_str() == Some(&login_id))
                        {
                            if job.state["auth"] == "signedIn" {
                                "completed"
                            } else {
                                let record = CodeSubmissionRecord {
                                    status: CodeSubmissionStatus::Accepted,
                                    login_id: login_id.clone(),
                                    reason: None,
                                };
                                job.code_submission = Some(record.clone());
                                job.state["codeSubmission"] =
                                    serde_json::to_value(&record).unwrap_or(Value::Null);
                                "accepted"
                            }
                        } else {
                            "accepted"
                        }
                    };
                    publish(svc, tab);
                    let mut response = show(svc, tab)?;
                    response["outcome"] = json!(outcome);
                    return Ok(response);
                }
                process::CodeWriteOutcome::FailedBeforeWrite(reason) => {
                    {
                        let mut jobs = JOBS.lock();
                        if let Some(job) = jobs
                            .get_mut(tab)
                            .filter(|job| job.state["login"]["loginId"].as_str() == Some(&login_id))
                        {
                            let record = CodeSubmissionRecord {
                                status: CodeSubmissionStatus::Failed,
                                login_id: login_id.clone(),
                                reason: Some(reason.clone()),
                            };
                            job.code_submission = Some(record.clone());
                            job.state["codeSubmission"] =
                                serde_json::to_value(&record).unwrap_or(Value::Null);
                        }
                    }
                    publish(svc, tab);
                    return Err(reason);
                }
                process::CodeWriteOutcome::Uncertain(reason) => {
                    {
                        let mut jobs = JOBS.lock();
                        if let Some(job) = jobs
                            .get_mut(tab)
                            .filter(|job| job.state["login"]["loginId"].as_str() == Some(&login_id))
                        {
                            let record = CodeSubmissionRecord {
                                status: CodeSubmissionStatus::Uncertain,
                                login_id: login_id.clone(),
                                reason: Some(reason.clone()),
                            };
                            job.code_submission = Some(record.clone());
                            job.state["codeSubmission"] =
                                serde_json::to_value(&record).unwrap_or(Value::Null);
                        }
                    }
                    publish(svc, tab);
                    let mut response = show(svc, tab)?;
                    response["outcome"] = json!("uncertain");
                    response["reason"] = json!(reason);
                    return Ok(response);
                }
            }
        }
        Some("cancel") => {
            let mut jobs = JOBS.lock();
            let job = jobs.get_mut(tab).ok_or("No pending CLI sign-in")?;
            require_challenge(svc, tab, job, &request)?;
            if let Some(stop) = &job.stop {
                let _ = stop.send(true);
            }
            job.input = None;
            job.code_submission = None;
            if let Some(obj) = job.state.as_object_mut() {
                obj.remove("codeSubmission");
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
            require_challenge(svc, tab, job, &request)?;
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
    if request.api_key.is_some() || request.code.is_some() {
        return Err("Sign-in start cannot carry credentials or codes".into());
    }
    let _permit = tokio::time::timeout(Duration::from_secs(30), START_GATE.acquire())
        .await
        .map_err(|_| "Sign-in request timed out")?
        .map_err(|_| "Sign-in unavailable")?;
    let owner_key = (provider.id().to_owned(), account.clone());
    if ACTIVE.lock().contains(&owner_key) {
        return Err(
            "A sign-in is already pending for this account. Cancel it before trying again.".into(),
        );
    }
    if RUNS
        .lock()
        .values()
        .any(|run| run.provider == provider.id() && run.account == account)
    {
        return Err("A sign-in process is still finishing for this account".into());
    }
    let signed_in = authenticated(provider.clone()).await?;
    if tab_identity(svc, tab)? != (account.clone(), nonce.clone(), session_id.clone()) {
        return Err("Sign-in tab binding changed during authentication".into());
    }
    let epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);
    let installation_id = local_installation_id(svc);
    let binding = build_login_binding(
        svc,
        tab,
        provider.id(),
        session_id.as_deref(),
        nonce.as_deref().unwrap_or(""),
        epoch,
    );
    let mut snapshot = state(provider.as_ref(), &cap, signed_in, epoch);
    snapshot["loginBinding"] = serde_json::to_value(&binding).unwrap_or(Value::Null);
    JOBS.lock().insert(
        tab.into(),
        Job {
            installation_id,
            provider: provider.id().into(),
            account: account.clone(),
            session_id: session_id.clone(),
            nonce: nonce.clone(),
            epoch,
            expires_at_ms: None,
            code_submission: None,
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
    if request.account.is_some()
        && request.method_id.is_some()
        && request.account != request.method_id
    {
        return Err("Conflicting CLI sign-in method selection".into());
    }
    let selected_choice = request.account.as_deref().or(request.method_id.as_deref());
    let browser_requested = matches!(
        request.method,
        Some(codeterm_plugin::agent_runtime::RuntimeLoginMethod::Browser)
    ) || (request.method.is_none() && selected_choice.is_some());
    if !browser_requested {
        return Err("This provider supports browser sign-in".into());
    }
    let method = match selected_choice {
        Some(id) => cap.methods.iter().find(|method| method.id == id),
        None => cap.methods.first(),
    }
    .ok_or("Unsupported provider sign-in method")?;
    if !ACTIVE.lock().insert(owner_key.clone()) {
        return Err("A provider sign-in is already pending".into());
    }
    let permit = Permit(owner_key);
    let (stop, stopping) = watch::channel(false);
    let (completed, finished) = watch::channel(false);
    let (input, codes) = mpsc::channel(1);
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at_ms = now_ms().saturating_add(cap.timeout_ms.clamp(1000, 900_000));
    {
        let mut jobs = JOBS.lock();
        let job = jobs.get_mut(tab).ok_or("Sign-in tab is unavailable")?;
        RUNS.lock().insert(
            tab.into(),
            Arc::new(Run {
                id: id.clone(),
                provider: provider.id().into(),
                account: account.clone(),
                nonce: nonce.clone(),
                stop: stop.clone(),
                finished,
            }),
        );
        job.stop = Some(stop);
        job.input = Some(input);
        job.expires_at_ms = Some(expires_at_ms);
        job.state["login"] =
            json!({"status":"pending","loginId":id,"method":"browser","expiresAtMs":expires_at_ms});
    }
    publish(svc, tab);
    let owner = svc.clone();
    let target = tab.to_owned();
    let response = show(svc, tab)?;
    let args = method.args.clone();
    let invocation = codeterm_plugin::provider_account::current();
    crate::async_runtime::spawn(codeterm_plugin::provider_account::scope(
        invocation,
        async move {
            let permit = permit;
            let completion = Completion(completed);
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
            let cleanup_confirmed = result.cleanup_confirmed;
            let verified = match result.outcome {
                Ok(()) => authenticated(provider).await,
                Err(error) => Err(error),
            };
            finish(&owner, &target, &id, verified);
            if cleanup_confirmed {
                drop(permit);
                completion.complete();
                let mut runs = RUNS.lock();
                if runs.get(&target).is_some_and(|run| run.id == id) {
                    runs.remove(&target);
                }
            }
        },
    ));
    Ok(response)
}

fn finish(svc: &ServiceContext, tab: &str, id: &str, result: Result<bool, String>) {
    let mut jobs = JOBS.lock();
    let Some(job) = jobs.get_mut(tab).filter(|job| {
        job.state["login"]["loginId"] == id
            && job.stop.is_some()
            && svc
                .mux
                .read()
                .get_tab(tab)
                .is_some_and(|target| matches_tab(job, target))
    }) else {
        return;
    };
    job.stop = None;
    job.input = None;
    if job.state["login"]["status"] == "canceled" {
        return;
    }
    let success = matches!(result, Ok(true));
    let failure_error = (!success).then(|| {
        result
            .err()
            .unwrap_or_else(|| "The provider is still signed out. Try again.".into())
    });
    if let Some(submission) = job.code_submission.as_mut() {
        if success {
            submission.status = CodeSubmissionStatus::Completed;
            submission.reason = None;
        } else if submission.status != CodeSubmissionStatus::Uncertain {
            submission.status = CodeSubmissionStatus::Failed;
            submission.reason = failure_error.clone();
        }
        job.state["codeSubmission"] = serde_json::to_value(submission).unwrap_or(Value::Null);
    }
    job.state["state"] = json!(if success { "loaded" } else { "loginPending" });
    job.state["auth"] = json!(if success { "signedIn" } else { "signedOut" });
    job.state["login"] = if success {
        json!({"status":"completed","loginId":id})
    } else {
        json!({"status":"failed","loginId":id,"error":failure_error.unwrap_or_else(|| "The provider is still signed out. Try again.".into())})
    };
    drop(jobs);
    publish(svc, tab);
}

fn observe(svc: &ServiceContext, tab: &str, id: &str, url: String) {
    let mut jobs = JOBS.lock();
    let Some(job) = jobs.get_mut(tab).filter(|job| {
        job.state["login"]["loginId"] == id
            && job.stop.is_some()
            && svc
                .mux
                .read()
                .get_tab(tab)
                .is_some_and(|target| matches_tab(job, target))
    }) else {
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
