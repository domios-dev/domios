use super::State;
use crate::service::ServiceContext;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct PendingClose {
    session: Arc<crate::pty::manager::PtySession>,
    nonce: Option<String>,
}

pub(super) type PendingCloses = BTreeMap<String, PendingClose>;

pub(crate) fn prepare(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    let target = svc
        .mux
        .read()
        .get_tab(tab)
        .cloned()
        .ok_or("The account tab is unavailable")?;
    let Some(provider) = target.agent_type.as_deref() else {
        return Ok(());
    };
    if super::selected_account(&target) == super::IMPLICIT_ACCOUNT
        || super::capability(svc, provider).is_none()
    {
        return Ok(());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let previous = state.closing.lock().get(tab).cloned();
    if let Some(previous) = previous {
        if previous.nonce != target.session_binding_nonce {
            return Err("The account process binding changed; credentials were retained".into());
        }
        previous
            .session
            .teardown
            .wait_for_completion(Instant::now() + Duration::from_secs(30))?;
        state.closing.lock().remove(tab);
        return Ok(());
    }
    let Some(pty) = target.pty_id.as_deref() else {
        return Ok(());
    };
    let (session, reaper) = {
        let mut manager = svc.pty.lock();
        let Some(session) = manager.get_session(pty) else {
            return Ok(());
        };
        let reaper = manager.begin_kill_tree(pty)?;
        (session, reaper)
    };
    state.closing.lock().insert(
        tab.into(),
        PendingClose {
            session,
            nonce: target.session_binding_nonce,
        },
    );
    reaper.wait_bounded(Duration::from_secs(30))?;
    state.closing.lock().remove(tab);
    Ok(())
}

pub(crate) fn closed(svc: &ServiceContext, tab: &str) {
    let Some(state) = svc.try_ext::<State>() else {
        return;
    };
    {
        let mut scheduler = state.scheduler.lock();
        scheduler.retire(tab);
    }
    let providers: std::collections::BTreeSet<_> = state
        .admissions
        .lock()
        .removing
        .keys()
        .map(|(provider, _)| provider.clone())
        .collect();
    for provider in providers {
        super::removal::schedule(svc, &provider);
    }
}
