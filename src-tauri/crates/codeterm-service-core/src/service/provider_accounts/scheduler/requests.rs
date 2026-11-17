use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectRequest {
    pub provider_id: String,
    pub account_id: String,
    pub previous_account_id: String,
    pub account_namespace: Option<String>,
    pub nonce: String,
    pub epoch: Option<u64>,
    pub revision: String,
    #[serde(default)]
    pub resume_work: bool,
    pub turn_id: Option<String>,
}

pub fn select(
    svc: &ServiceContext,
    tab: &str,
    request: SelectRequest,
    verify: bool,
) -> Result<Transition, String> {
    let expected = capture(svc, tab)?;
    let provider = &expected.provider;
    let policy = policy(svc, provider);
    if expected.provider != request.provider_id
        || expected
            .account_id
            .as_deref()
            .unwrap_or(super::super::IMPLICIT_ACCOUNT)
            != request.previous_account_id
        || expected.account_namespace != request.account_namespace
        || expected.binding_nonce != request.nonce
        || expected.runtime_epoch != request.epoch
        || super::super::policy::revision(&policy)? != request.revision
    {
        return Err("The tab account changed. Refresh it before trying again.".into());
    }
    if !policy.order.contains(&request.account_id) {
        return Err("The selected harness account no longer exists".into());
    }
    if !verify
        && super::super::continuation(svc, provider)
            != codeterm_plugin::provider_account::storage::AccountContinuation::Proven
    {
        return Err("Moving this conversation to another account isn't confirmed yet.".into());
    }
    if request.account_id == request.previous_account_id {
        return Err("This tab already uses that account".into());
    }
    if request.resume_work
        && (verify
            || !request
                .turn_id
                .as_deref()
                .is_some_and(|turn| account_turn_boundary(svc, tab, &expected, turn)))
    {
        return Err("The conversation no longer has this exact failed-turn boundary".into());
    }
    if verify {
        super::super::receipt::validate_accounts(
            svc,
            provider,
            &request.previous_account_id,
            &request.account_id,
        )?;
    }
    let state = svc.ext_or_init::<State>(State::default);
    let claim = {
        let mut scheduler = state.scheduler.lock();
        hydrate(svc, provider, &mut scheduler.book);
        let source = binding(&expected, tab, &scheduler.book);
        scheduler.book.bind(source.clone());
        let turn = request
            .turn_id
            .clone()
            .filter(|_| request.resume_work)
            .unwrap_or_else(|| format!("manual-{}", uuid::Uuid::new_v4()));
        let decision = scheduler.book.request(&source, &request.account_id, &turn);
        match decision {
            Decision::Switch(claim) => claim,
            _ => return Err("Sign in to an available account before switching this tab".into()),
        }
    };
    let pending = match prepare(
        svc,
        tab,
        claim.clone(),
        expected.clone(),
        request.resume_work,
        verify,
    ) {
        Ok(pending) => pending,
        Err(error) => {
            state.scheduler.lock().book.cancel(&claim);
            return Err(error);
        }
    };
    let transition = Transition {
        cause: if request.resume_work {
            "limit"
        } else if verify {
            "verification"
        } else {
            "manual"
        },
        account_identity: identity(svc, provider, &request.previous_account_id),
        id: uuid::Uuid::new_v4().to_string(),
        provider_id: provider.clone(),
        account_id: request.previous_account_id,
        status: "waiting",
        next_identity: Some(identity(svc, provider, &request.account_id)),
        next_account_id: Some(request.account_id),
        resets_at_ms: None,
        turn_id: request.turn_id.filter(|_| request.resume_work),
        reason: None,
        continuation: super::super::continuation(svc, provider),
    };
    {
        let mut scheduler = state.scheduler.lock();
        scheduler.pending.insert(tab.into(), pending);
        scheduler.transitions.insert(tab.into(), transition.clone());
    }
    emit(svc, tab);
    dispatch(svc, tab);
    Ok(transition)
}

pub(in crate::service::provider_accounts) fn evacuate(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
) {
    if super::super::continuation(svc, provider)
        != codeterm_plugin::provider_account::storage::AccountContinuation::Proven
    {
        return;
    }
    let tabs: Vec<_> = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && super::super::selected_account(tab) == account
                && tab.description.as_deref() != Some("domios:provider-account-sign-in")
        })
        .map(|tab| tab.id.clone())
        .collect();
    for tab in tabs {
        let Ok(expected) = capture(svc, &tab) else {
            continue;
        };
        let policy = policy(svc, provider);
        let state = svc.ext_or_init::<State>(State::default);
        let claim = {
            let mut scheduler = state.scheduler.lock();
            if scheduler.pending.contains_key(&tab) {
                continue;
            }
            hydrate(svc, provider, &mut scheduler.book);
            let target = policy
                .next_eligible(account, |id| {
                    super::super::admission::available(svc, provider, id)
                        && scheduler.book.authentication(provider, id)
                            == Some(Authentication::Ready)
                })
                .ok()
                .flatten()
                .map(str::to_owned);
            let Some(target) = target else {
                continue;
            };
            let source = binding(&expected, &tab, &scheduler.book);
            scheduler.book.bind(source.clone());
            let turn = scheduler
                .boundaries
                .get(&tab)
                .filter(|boundary| boundary.expected == expected)
                .map(|boundary| boundary.turn.clone())
                .unwrap_or_else(|| format!("remove-{}", uuid::Uuid::new_v4()));
            match scheduler.book.request(&source, &target, &turn) {
                Decision::Switch(claim) => claim,
                _ => continue,
            }
        };
        let pending = match prepare(svc, &tab, claim.clone(), expected, false, false) {
            Ok(pending) => pending,
            Err(_) => {
                state.scheduler.lock().book.cancel(&claim);
                continue;
            }
        };
        let transition = Transition {
            cause: "removal",
            account_identity: identity(svc, provider, account),
            id: uuid::Uuid::new_v4().to_string(),
            provider_id: provider.into(),
            account_id: account.into(),
            status: "waiting",
            next_identity: Some(identity(svc, provider, &claim.target)),
            next_account_id: Some(claim.target),
            resets_at_ms: None,
            turn_id: None,
            reason: None,
            continuation: super::super::continuation(svc, provider),
        };
        {
            let mut scheduler = state.scheduler.lock();
            scheduler.pending.insert(tab.clone(), pending);
            scheduler.transitions.insert(tab.clone(), transition);
        }
        emit(svc, &tab);
        dispatch(svc, &tab);
    }
}

pub(in crate::service::provider_accounts) fn reauthenticated(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
) {
    if account == super::super::IMPLICIT_ACCOUNT {
        return;
    }
    let tabs: Vec<_> = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && super::super::selected_account(tab) == account
                && tab.description.as_deref() != Some("domios:provider-account-sign-in")
        })
        .map(|tab| tab.id.clone())
        .collect();
    for tab in tabs {
        let result = (|| {
            let namespace = super::super::credential_namespace(svc, provider, Some(account), None)?;
            let expected = match capture(svc, &tab) {
                Ok(expected) => expected,
                Err(_) => AccountRestartBinding::capture_refresh(svc, &tab).or_else(|_| {
                    let epoch = runtime_epoch(svc, &tab)
                        .ok_or("Start a new tab to use this saved sign-in")?;
                    let mux = svc.mux.read();
                    AccountRestartBinding::capture_login(
                        mux.get_tab(&tab).ok_or("The account tab closed")?,
                        epoch,
                    )
                })?,
            };
            if expected.account_namespace == namespace {
                return Ok(());
            }
            let state = svc.ext_or_init::<State>(State::default);
            let claim = {
                let mut scheduler = state.scheduler.lock();
                hydrate(svc, provider, &mut scheduler.book);
                let source = binding(&expected, &tab, &scheduler.book);
                scheduler.book.bind(source.clone());
                match scheduler.book.request(
                    &source,
                    account,
                    &format!("reauth-{}", uuid::Uuid::new_v4()),
                ) {
                    Decision::Switch(claim) => claim,
                    _ => return Err("This tab is already changing accounts".into()),
                }
            };
            let mut pending = match prepare(svc, &tab, claim.clone(), expected, false, false) {
                Ok(pending) => pending,
                Err(error) => {
                    state.scheduler.lock().book.cancel(&claim);
                    return Err(error);
                }
            };
            pending.target.terminal_turn = None;
            let transition = Transition {
                cause: "reauth",
                account_identity: identity(svc, provider, account),
                id: uuid::Uuid::new_v4().to_string(),
                provider_id: provider.into(),
                account_id: account.into(),
                status: "waiting",
                next_account_id: Some(account.into()),
                next_identity: Some(identity(svc, provider, account)),
                resets_at_ms: None,
                turn_id: None,
                reason: None,
                continuation: super::super::continuation(svc, provider),
            };
            {
                let mut scheduler = state.scheduler.lock();
                scheduler.pending.insert(tab.clone(), pending);
                scheduler.transitions.insert(tab.clone(), transition);
            }
            emit(svc, &tab);
            dispatch(svc, &tab);
            Ok::<_, String>(())
        })();
        if let Err(reason) = result {
            svc.ext_or_init::<State>(State::default)
                .scheduler
                .lock()
                .transitions
                .insert(
                    tab.clone(),
                    Transition {
                        cause: "reauth",
                        account_identity: identity(svc, provider, account),
                        id: uuid::Uuid::new_v4().to_string(),
                        provider_id: provider.into(),
                        account_id: account.into(),
                        status: "failed",
                        next_account_id: Some(account.into()),
                        next_identity: Some(identity(svc, provider, account)),
                        resets_at_ms: None,
                        turn_id: None,
                        reason: Some(reason),
                        continuation: super::super::continuation(svc, provider),
                    },
                );
            emit(svc, &tab);
        }
    }
}
