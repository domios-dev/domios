use super::{
    engine::{self, Session},
    model::{RuntimeFailure, RuntimePhase},
    restart,
};
use crate::service::ServiceContext;
use std::sync::{atomic::Ordering, Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Wait,
    Restart,
    Observe,
    Stop,
}

fn decide(current: bool, busy: bool, failed: bool, restarted: bool, recovered: bool) -> Decision {
    if !current {
        return Decision::Stop;
    }
    if busy {
        return Decision::Wait;
    }
    if !restarted || (failed && !recovered) {
        return Decision::Restart;
    }
    if failed {
        Decision::Stop
    } else {
        Decision::Observe
    }
}

fn version_mismatch(actual: Option<&str>, expected: &str) -> bool {
    actual.is_some_and(|actual| actual != expected)
}

pub(super) fn mask_recoverable_failure(
    snapshot: &mut super::model::RuntimeSnapshot,
    recovering: bool,
) {
    if recovering && matches!(snapshot.state.as_str(), "degraded" | "unloaded") {
        snapshot.state = "loading".into();
        snapshot.reason = None;
    }
}

pub(super) fn busy(session: &Session) -> bool {
    let context = session.context.lock();
    context.active_turn_id.is_some() || !context.active_turns.is_empty()
}

pub(crate) fn schedule(svc: &ServiceContext, tab: &str, version: &str) {
    let Some(session) = engine::for_tab(tab) else {
        return;
    };
    let generation = session.update_generation.fetch_add(1, Ordering::AcqRel) + 1;
    session
        .update_recovery_available
        .store(true, Ordering::Release);
    let changes = session.control_changes.subscribe();
    let owner = svc.clone();
    let tab = tab.to_string();
    let version = version.to_string();
    crate::async_runtime::spawn(async move {
        monitor(owner, tab, version, session, generation, changes).await;
    });
}

async fn monitor(
    svc: ServiceContext,
    tab: String,
    version: String,
    session: Arc<Session>,
    generation: u64,
    mut changes: tokio::sync::watch::Receiver<()>,
) {
    let thread = session.context.lock().thread_id.clone();
    let mut restarted = false;
    let mut recovered = false;
    let mut failure = None;
    loop {
        changes.borrow_and_update();
        let current = session.update_generation.load(Ordering::Acquire) == generation
            && engine::for_tab(&tab).is_some_and(|current| Arc::ptr_eq(&current, &session))
            && engine::validate_binding(&svc, &session).is_ok()
            && session.context.lock().thread_id == thread;
        let mismatch = version_mismatch(session.state.lock().binary_version.as_deref(), &version);
        let failed = failure.is_some()
            || (restarted && mismatch)
            || matches!(
                session.state.lock().phase,
                RuntimePhase::Degraded { .. } | RuntimePhase::Unloaded { .. }
            );
        match decide(current, busy(&session), failed, restarted, recovered) {
            Decision::Stop => {
                if session.update_generation.load(Ordering::Acquire) == generation {
                    session
                        .update_recovery_available
                        .store(false, Ordering::Release);
                }
                if current && failed {
                    let reason = failure.unwrap_or_else(|| RuntimeFailure::Refused(
                        format!("Runtime failed after provider update to {version}; automatic recovery exhausted (reported version: {:?})", session.state.lock().binary_version)));
                    let epoch = session.state.lock().epoch;
                    session.state.lock().degrade(epoch, &reason);
                    engine::publish(&svc, &tab, &session);
                }
                return;
            }
            Decision::Restart => {
                let nonce = session.context.lock().binding_nonce.clone();
                match restart::restart_idle(&svc, &tab, &session.provider, &nonce).await {
                    Ok(None) => {}
                    result => {
                        recovered |= restarted;
                        restarted = true;
                        failure = result.err();
                        if recovered
                            && failure.is_none()
                            && !version_mismatch(
                                session.state.lock().binary_version.as_deref(),
                                &version,
                            )
                        {
                            session
                                .update_recovery_available
                                .store(false, Ordering::Release);
                            engine::publish(&svc, &tab, &session);
                        }
                        if failure.is_none() {
                            let owner = svc.clone();
                            let target = tab.clone();
                            let thread = thread.clone();
                            match tokio::task::spawn_blocking(move || {
                                owner.restore_caller_binding(&target, thread.as_deref())
                            }).await {
                                Ok(Ok(_)) => {}
                                result => log::warn!("[provider-runtime-update] tab={tab} caller restoration failed: {result:?}"),
                            }
                            svc.schedule_session_persist_debounced();
                        }
                        log::debug!("[provider-runtime-update] tab={tab} target={version} recovery={recovered} failed={}", failure.is_some());
                        continue;
                    }
                }
            }
            Decision::Wait | Decision::Observe => {}
        }
        if changes.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
