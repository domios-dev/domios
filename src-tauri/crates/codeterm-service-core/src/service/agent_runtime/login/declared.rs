use super::*;

#[derive(Clone)]
pub(super) struct Claim {
    method: String,
    id: String,
    nonce: String,
    epoch: u64,
    thread_id: Option<String>,
    callback_started: bool,
}

fn method(session: &Session, id: &str) -> Option<RuntimeAuthenticationMethod> {
    session
        .maps
        .lock()
        .authentication
        .as_ref()?
        .methods
        .iter()
        .find(|method| method.id == id && method.input.is_some())
        .cloned()
}

pub(super) fn handles(session: &Session, request: &LoginRequest) -> bool {
    request
        .method_id
        .as_deref()
        .is_some_and(|id| method(session, id).is_some())
        || (request.method_id.is_none()
            && session.login.lock().declared.is_some()
            && matches!(
                request.action.as_deref(),
                Some("cancel" | "code" | "notify")
            ))
}

fn current(claim: &Claim, login: Option<&RuntimeLoginState>, context: &RuntimeContext) -> bool {
    current_at(claim, login, context, now_ms())
}

fn current_at(
    claim: &Claim,
    login: Option<&RuntimeLoginState>,
    context: &RuntimeContext,
    now: u64,
) -> bool {
    owns(claim, login, context)
        && login.is_some_and(|login| login.expires_at_ms.is_none_or(|expiry| expiry > now))
}

fn owns(claim: &Claim, login: Option<&RuntimeLoginState>, context: &RuntimeContext) -> bool {
    claim.nonce == context.binding_nonce
        && claim.epoch == context.epoch
        && claim.thread_id == context.thread_id
        && login.is_some_and(|login| {
            login.status == RuntimeLoginStatus::Pending
                && login.login_id.as_deref() == Some(claim.id.as_str())
        })
}

impl Claim {
    fn begin_callback(
        &mut self,
        login: Option<&RuntimeLoginState>,
        context: &RuntimeContext,
        now: u64,
        code_input: bool,
    ) -> bool {
        if !current_at(self, login, context, now)
            || self.callback_started
            || !login.is_some_and(|login| {
                login.code_input == Some(code_input) && (code_input || login.url.is_some())
            })
        {
            return false;
        }
        self.callback_started = true;
        true
    }
}

pub(super) fn accepts(session: &Session, event: &RuntimeLoginState) -> bool {
    let context = session.context.lock().clone();
    let state = session.login.lock();
    state.declared.as_ref().is_none_or(|claim| {
        current(claim, state.login.as_ref(), &context)
            && event.login_id.as_deref() == Some(claim.id.as_str())
    })
}

pub(super) fn validate_witness(
    session: &Session,
    witness: &(String, String),
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let state = session.login.lock();
    if state.declared.as_ref().is_some_and(|claim| {
        claim.method == witness.0
            && claim.id == witness.1
            && current(claim, state.login.as_ref(), &context)
    }) {
        Ok(())
    } else {
        Err(RuntimeFailure::Superseded)
    }
}

pub(super) async fn operate(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    request: LoginRequest,
) -> Result<Value, String> {
    validate_request(session, &request)?;
    if request.action.as_deref() == Some("cancel") {
        let context = session.context.lock().clone();
        {
            let mut state = session.login.lock();
            let claim = state.declared.as_ref().ok_or("No sign-in is pending")?;
            if !owns(claim, state.login.as_ref(), &context)
                || request.login_id.as_deref() != Some(claim.id.as_str())
            {
                return Err("Sign-in challenge is no longer current".into());
            }
            let login = state.login.as_mut().ok_or("No sign-in is pending")?;
            login.status = RuntimeLoginStatus::Canceled;
            login.url = None;
            login.user_code = None;
            login.instructions = None;
            login.code_input = None;
            login.expires_at_ms = None;
        }
        forget(session);
        engine::publish(svc, tab, session);
        return show(svc, tab);
    }
    if request.action.as_deref() == Some("notify") {
        if request.login_id.is_none() {
            return Err("Current sign-in challenge required".into());
        }
        return relay::notify(svc, tab, session);
    }
    let _permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| "Sign-in deadline exceeded")?
        .map_err(|_| "Runtime unavailable")?;
    engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
    validate_request(session, &request)?;
    let context = session.context.lock().clone();
    let value = if request.action.as_deref() == Some("code") {
        if request.api_key.is_some() || request.method_id.is_some() || request.method.is_some() {
            return Err("Invalid browser-code response".into());
        }
        let code = request
            .code
            .as_deref()
            .filter(|code| !code.trim().is_empty() && code.len() <= 16384)
            .ok_or("Browser code required (maximum 16 KiB)")?;
        let mut state = session.login.lock();
        let claim = state.declared.as_ref().ok_or("No sign-in is pending")?;
        if request.login_id.as_deref() != Some(claim.id.as_str()) {
            return Err("Sign-in challenge cannot accept this code".into());
        }
        let login = state.login.clone();
        let claim = state.declared.as_mut().ok_or("No sign-in is pending")?;
        if !claim.begin_callback(login.as_ref(), &context, now_ms(), true) {
            return Err("Sign-in challenge cannot accept this code".into());
        }
        json!({"action":"code","methodId":claim.method,"loginId":claim.id,"code":code})
    } else {
        if request
            .action
            .as_deref()
            .is_some_and(|action| action != "start")
            || request.method.is_some()
            || request.code.is_some()
        {
            return Err("Invalid declared sign-in request".into());
        }
        let id = request
            .method_id
            .as_deref()
            .ok_or("Authentication method required")?;
        let method = method(session, id)
            .filter(|method| method.available)
            .ok_or("Authentication method was not declared or is unavailable")?;
        let takes_key = method.input == Some(RuntimeAuthenticationInput::ApiKey);
        if takes_key
            && request
                .api_key
                .as_deref()
                .is_none_or(|key| key.trim().is_empty() || key.len() > 16384)
        {
            return Err("API key required (maximum 16 KiB)".into());
        }
        if !takes_key && request.api_key.is_some() {
            return Err("This authentication method does not take a key".into());
        }
        claim(session, tab)?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut state = session.login.lock();
        state.phone_notification = None;
        state.declared = Some(Claim {
            method: method.id.clone(),
            id: id.clone(),
            nonce: context.binding_nonce.clone(),
            epoch: context.epoch,
            thread_id: context.thread_id.clone(),
            callback_started: takes_key,
        });
        state.login = Some(RuntimeLoginState {
            status: RuntimeLoginStatus::Pending,
            login_id: Some(id.clone()),
            method: Some(if takes_key {
                RuntimeLoginMethod::ApiKey
            } else {
                RuntimeLoginMethod::Browser
            }),
            instructions: None,
            code_input: None,
            url: None,
            user_code: None,
            expires_at_ms: Some(now_ms().saturating_add(15 * 60 * 1000)),
            error: None,
        });
        json!({"action":"start","methodId":method.id,"loginId":id,"apiKey":request.api_key})
    };
    engine::publish(svc, tab, session);
    let witness = {
        let state = session.login.lock();
        let claim = state
            .declared
            .as_ref()
            .ok_or("Sign-in ownership unavailable")?;
        (claim.method.clone(), claim.id.clone())
    };
    let secret = request.api_key.as_deref().or(request.code.as_deref());
    run(svc, session, &context, value)
        .await
        .map_err(|failure| fail(svc, session, &context, &witness, failure, secret))?;
    show(svc, tab)
}

async fn run(
    svc: &ServiceContext,
    session: &Arc<Session>,
    context: &RuntimeContext,
    value: Value,
) -> Result<(), RuntimeFailure> {
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let result = engine::run(
        svc,
        &session.provider,
        session,
        &process,
        RuntimeOperation::Control {
            thread_id: context.thread_id.clone(),
            action: "login".into(),
            value,
        },
    )
    .await?;
    if !matches!(result, RuntimeResult::Done) {
        return Err(RuntimeFailure::Refused(
            "Unsupported sign-in response".into(),
        ));
    }
    Ok(())
}

fn fail(
    svc: &ServiceContext,
    session: &Arc<Session>,
    context: &RuntimeContext,
    witness: &(String, String),
    failure: RuntimeFailure,
    secret: Option<&str>,
) -> String {
    let reason = safe_reason(&run_failure_reason(&failure), secret);
    let live = session.context.lock().clone();
    if live.binding_nonce != context.binding_nonce || live.epoch != context.epoch {
        return reason;
    }
    let mut state = session.login.lock();
    if state.declared.as_ref().is_some_and(|claim| {
        claim.method == witness.0
            && claim.id == witness.1
            && current(claim, state.login.as_ref(), &live)
    }) {
        if let Some(login) = state.login.as_mut() {
            login.status = RuntimeLoginStatus::Failed;
            login.url = None;
            login.instructions = None;
            login.code_input = None;
            login.error = Some(reason.clone());
        }
        drop(state);
        forget(session);
        engine::publish(svc, &context.tab_id, session);
    }
    reason
}

pub(super) fn start_callback(svc: &ServiceContext, session: &Arc<Session>) {
    let context = session.context.lock().clone();
    let witness = {
        let mut state = session.login.lock();
        let login = state.login.clone();
        let Some(claim) = state.declared.as_mut() else {
            return;
        };
        if !claim.begin_callback(login.as_ref(), &context, now_ms(), false) {
            return;
        }
        (claim.method.clone(), claim.id.clone())
    };
    let svc = svc.clone();
    let session = session.clone();
    crate::async_runtime::spawn(async move {
        let result = async {
            let _permit = tokio::time::timeout(Duration::from_secs(30), session.gate.acquire())
                .await
                .map_err(|_| RuntimeFailure::Deadline)?
                .map_err(|_| RuntimeFailure::Superseded)?;
            engine::validate_binding(&svc, &session)?;
            validate_witness(&session, &witness)?;
            run(
                &svc,
                &session,
                &context,
                json!({"action":"complete","methodId":witness.0,"loginId":witness.1}),
            )
            .await
        }
        .await;
        if let Err(error) = result {
            if validate_witness(&session, &witness).is_ok() {
                fail(&svc, &session, &context, &witness, error, None);
            }
        }
    });
}

#[cfg(test)]
mod tests;
