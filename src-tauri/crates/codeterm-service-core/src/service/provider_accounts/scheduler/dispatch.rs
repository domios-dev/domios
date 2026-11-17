use super::*;
use crate::service::agent_lifecycle::RestartOptions;

pub(in crate::service::provider_accounts) fn dispatch(svc: &ServiceContext, tab: &str) {
    let Some(state) = svc.try_ext::<State>() else {
        return;
    };
    let pending = {
        let mut scheduler = state.scheduler.lock();
        let Some(pending) = scheduler.pending.get(tab) else {
            return;
        };
        if pending.resume_work
            && ((pending.automatic
                && !policy(svc, &pending.claim.source.provider).automatic_switch)
                || super::super::continuation(svc, &pending.claim.source.provider)
                    != codeterm_plugin::provider_account::storage::AccountContinuation::Proven)
        {
            let pending = scheduler.pending.remove(tab).unwrap();
            scheduler.book.cancel(&pending.claim);
            if let Some(transition) = scheduler.transitions.get_mut(tab) {
                transition.status = "exhausted";
            }
            return;
        }
        if !scheduler.book.current(&pending.claim) || !bound(svc, tab, &pending.target.expected) {
            let pending = scheduler.pending.remove(tab).unwrap();
            scheduler.book.cancel(&pending.claim);
            return;
        }
        if let Some(session) = engine::for_tab(tab) {
            let context = session.context.lock();
            if context.active_turn_id.is_some()
                || !context.active_turns.is_empty()
                || !session.turn_requests.lock().is_empty()
                || (pending.resume_work
                    && !session
                        .completions
                        .lock()
                        .contains(&pending.claim.source.session, &pending.claim.turn))
            {
                return;
            }
        } else {
            let idle = svc
                .try_snapshot_with_reconciled_status()
                .is_some_and(|snapshot| {
                    snapshot.tab_statuses.get(tab) == Some(&codeterm_types::pty::TabStatus::Idle)
                });
            if !idle
                && !scheduler.boundaries.get(tab).is_some_and(|boundary| {
                    boundary.expected == pending.target.expected
                        && boundary.turn == pending.claim.turn
                })
            {
                return;
            }
        }
        let pending = scheduler.pending.remove(tab).unwrap();
        if let Some(transition) = scheduler.transitions.get_mut(tab) {
            transition.status = "switching";
        }
        pending
    };
    emit(svc, tab);
    let owner = svc.clone();
    let tab = tab.to_owned();
    let receipt = super::super::work::Receipt::capture(svc);
    crate::async_runtime::spawn(async move {
        let worker = owner.clone();
        let target = tab.clone();
        let result = codeterm_plugin::js_runtime::spawn_blocking(move || {
            let _receipt = receipt;
            finish(&worker, &target, pending)
        })
        .await;
        if result.is_err() {
            failed(
                &owner,
                &tab,
                "The account transition could not finish".into(),
            );
        }
    });
}

fn finish(svc: &ServiceContext, tab: &str, pending: Pending) {
    let Pending {
        claim,
        target,
        reservation: reservation_guard,
        resume_work,
        automatic: _,
        verify,
    } = pending;
    let result = (|| {
        if !svc
            .ext_or_init::<State>(State::default)
            .scheduler
            .lock()
            .book
            .current(&claim)
        {
            return Err("The account transition was canceled".into());
        }
        let outcome = svc
            .lifecycle_port()
            .agent_restart_observed_with_options(
                tab,
                RestartOptions {
                    account_restart: Some(target.clone()),
                    ..Default::default()
                },
            )
            .map_err(|error| format!("Account restart failed: {error:?}"))?;
        if !outcome.agent_ready
            || !outcome.binding_restored
            || (!target.expected.unbound_login
                && outcome.session_id.as_deref() != Some(claim.source.session.as_str()))
            || outcome.session_was_stale
        {
            return Err(
                "The harness did not confirm the same conversation on the next account".into(),
            );
        }
        let resumed = capture(svc, tab)?;
        if resumed.provider != target.expected.provider
            || resumed.account_id != target.account_id
            || resumed.account_namespace != target.account_namespace
            || (!target.expected.unbound_login && resumed.session_id != target.expected.session_id)
            || resumed.binding_nonce == target.expected.binding_nonce
        {
            return Err("The account resume evidence does not match this tab".into());
        }
        if let Some(session) = engine::for_tab(tab) {
            if *session.auth.lock() != RuntimeAuthState::SignedIn
                || !engine::has_live_bound_session(
                    svc.mux
                        .read()
                        .get_tab(tab)
                        .ok_or("The account tab closed")?,
                )
            {
                return Err("The next account still needs sign-in".into());
            }
        }
        let state = svc.ext_or_init::<State>(State::default);
        let mut scheduler = state.scheduler.lock();
        let binding = binding(&resumed, tab, &scheduler.book);
        if target.expected.unbound_login {
            if !scheduler.book.current(&claim) || claim.target != claim.source.account {
                return Err("The sign-in claim changed during restart".into());
            }
            scheduler.book.cancel(&claim);
            scheduler.book.bind(binding);
        } else if !scheduler.book.finish(&claim, binding) {
            return Err("The account claim changed during restart".into());
        }
        scheduler.boundaries.remove(tab);
        drop(scheduler);
        append_notice(svc, tab);
        if verify {
            super::super::receipt::record(svc, &target.expected, &resumed)?;
        }
        if resume_work {
            svc.continue_after_account(
                tab,
                &resumed.session_id,
                &resumed.binding_nonce,
                &target.expected.binding_nonce,
            )?;
        }
        Ok::<_, String>(())
    })();
    if verify && result.is_err() {
        if let Err(reason) = super::super::receipt::record_refusal(svc, tab, &target) {
            log::debug!("[provider-accounts] live refusal receipt unavailable: {reason}");
        }
    }
    let state = svc.ext_or_init::<State>(State::default);
    {
        let mut scheduler = state.scheduler.lock();
        if let Err(reason) = result {
            scheduler.book.cancel(&claim);
            if let Some(transition) = scheduler.transitions.get_mut(tab) {
                transition.status = "failed";
                transition.reason = Some(reason);
                transition.continuation = super::super::continuation(svc, &claim.source.provider);
            }
        } else if let Some(transition) = scheduler.transitions.get_mut(tab) {
            transition.status = "continued";
            transition.continuation = super::super::continuation(svc, &claim.source.provider);
        }
    }
    drop(reservation_guard);
    super::super::removal::schedule(svc, &claim.source.provider);
    emit(svc, tab);
    svc.ui.emit(
        "provider-accounts-changed",
        serde_json::json!({"providerId":claim.source.provider}),
    );
}

fn append_notice(svc: &ServiceContext, tab: &str) {
    let Some(transition) = transition(svc, tab) else {
        return;
    };
    let mut mux = svc.mux.write();
    let Some(target) = mux.get_tab_mut(tab) else {
        return;
    };
    let Some(session) = target.agent_session_id.clone() else {
        return;
    };
    let next = transition
        .next_identity
        .as_deref()
        .unwrap_or("the next account");
    let content = if transition.cause == "reauth" {
        format!("Signed in as {next}.")
    } else if transition.cause == "limit" {
        format!(
            "Continued on {next} because {} reached its limit.",
            transition.account_identity
        )
    } else {
        format!("Continued on {next}.")
    };
    let notices = &mut target.restorable_launch.account_notices;
    if !notices.iter().any(|notice| notice.id == transition.id) {
        notices.push(crate::multiplexer::types::AccountContinuationNotice {
            id: transition.id,
            provider: transition.provider_id,
            session,
            at_ms: crate::delayed_send::now_ms(),
            content,
            boundary: transition.turn_id,
            resets_at_ms: transition.resets_at_ms,
        });
        if notices.len() > 32 {
            notices.drain(..notices.len() - 32);
        }
    }
    drop(mux);
    svc.schedule_session_persist_debounced();
    svc.emit_state_changed(crate::service::MuxEmitCause::TabLifecycle);
}

fn failed(svc: &ServiceContext, tab: &str, reason: String) {
    if let Some(state) = svc.try_ext::<State>() {
        if let Some(transition) = state.scheduler.lock().transitions.get_mut(tab) {
            transition.status = "failed";
            transition.reason = Some(reason);
        }
        emit(svc, tab);
    }
}
