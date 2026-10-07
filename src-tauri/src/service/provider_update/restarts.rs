use super::{
    state::{RestartState, RunOutcome, TabReadiness, UpdateRun},
    storage, ServiceContext,
};

pub(super) fn provider_tabs(service: &ServiceContext, provider: &str) -> Vec<String> {
    service
        .mux
        .read()
        .snapshot()
        .tabs
        .into_iter()
        .filter(|(_, tab)| tab.agent_type.as_deref() == Some(provider))
        .map(|(id, _)| id)
        .collect()
}

pub(super) fn readiness(service: &ServiceContext, tab_id: &str) -> TabReadiness {
    let active_attempt = service.plan_store.list_full().iter().any(|plan| {
        plan.tasks.iter().any(|task| {
            task.status == crate::plan::TaskStatus::InProgress
                && task
                    .execution_attempts
                    .iter()
                    .any(|attempt| attempt.executor_tab_id == tab_id && attempt.closed_at.is_none())
        })
    });
    let snapshot = service.snapshot_with_reconciled_status();
    if let Some(sidecar) = crate::service::codex_rpc::diagnostics::for_tab(tab_id) {
        let idle = crate::service::codex_rpc::diagnostics::restart_when_idle(
            sidecar.active_turn,
            sidecar.selection_verified,
            active_attempt,
        );
        return TabReadiness {
            idle,
            active_turn: sidecar.active_turn || !sidecar.selection_verified,
            active_attempt,
        };
    }
    let status = snapshot.tabs.get(tab_id).map(|tab| {
        crate::multiplexer::state::effective_tab_status(
            tab,
            snapshot.tab_statuses.get(tab_id).copied(),
        )
    });
    TabReadiness {
        idle: status == Some(codeterm_types::pty::TabStatus::Idle),
        active_turn: status.is_none_or(|status| status.has_active_turn()),
        active_attempt,
    }
    .with_background_work(!crate::service::background_work::running(service, tab_id).is_empty())
}

pub(super) fn schedule_async(service: &ServiceContext, tab: Option<String>) {
    let service = service.clone();
    super::spawn_utility("provider-update-idle", move || {
        schedule(&service, tab.as_deref())
    });
}

pub(super) fn schedule(service: &ServiceContext, only_tab: Option<&str>) {
    if let Ok(interrupted) =
        storage::transact(|state| state.recover_interrupted(super::command::owner_alive))
    {
        for (provider, target, tab) in interrupted {
            super::journal(
                &provider,
                Some(&target),
                tab.as_deref(),
                "interrupted",
                "provider_update_interrupted",
            );
        }
    }
    let Ok(state) = storage::read() else {
        return;
    };
    for (key, run) in state.runs {
        if run.outcome == RunOutcome::Succeeded {
            for (tab_id, restart) in &run.tabs {
                if only_tab.is_some_and(|tab| tab != tab_id) || *restart != RestartState::Pending {
                    continue;
                }
                let provider_matches =
                    service.mux.read().get_tab(tab_id).is_some_and(|tab| {
                        tab.agent_type.as_deref() == Some(run.provider.as_str())
                    });
                if !provider_matches {
                    continue;
                }
                let ready = readiness(service, tab_id);
                let claimed = storage::transact(|state| state.claim_restart(&key, tab_id, ready))
                    .unwrap_or(false);
                if claimed {
                    dispatch_restart(service, &key, tab_id, &run.provider);
                }
            }
        }
        refresh_summary(service, &key);
    }
}

fn dispatch_restart(service: &ServiceContext, key: &str, tab: &str, provider: &str) {
    let service = service.clone();
    let key = key.to_string();
    let tab = tab.to_string();
    let provider = provider.to_string();
    let failure_key = key.clone();
    let failure_tab = tab.clone();
    let builder = std::thread::Builder::new().name("provider-update-restart".into());
    let spawned = builder.spawn(move || restart_tab(&service, &key, &tab, &provider));
    if let Err(error) = spawned {
        log::error!("[provider-update] restart utility thread failed: {error}");
        let _ = storage::transact(|state| {
            state.settle_restart(&failure_key, &failure_tab, RestartState::Failed)
        });
    }
}

fn restart_tab(service: &ServiceContext, key: &str, tab: &str, provider: &str) {
    let target = key.strip_prefix(&format!("{provider}:"));
    let outcome = if super::journal(
        provider,
        target,
        Some(tab),
        "restart_requested",
        "provider_update_restart",
    ) {
        service.agent_restart_for_provider_update(tab, provider)
    } else {
        Err(super::super::agent_restart::AgentRestartError::Internal(
            "Supporter journal unavailable; restart withheld".into(),
        )
        .into())
    };
    if matches!(&outcome, Err(super::super::agent_restart::RestartAttemptError::Request(super::super::agent_restart::AgentRestartError::BadRequest(reason))) if reason == super::IDLE_REFUSAL)
        || matches!(
            &outcome,
            Err(super::super::agent_restart::RestartAttemptError::Request(
                super::super::agent_restart::AgentRestartError::BackgroundWork { .. }
            ))
        )
    {
        let _ = storage::transact(|state| state.defer_restart(key, tab));
        if matches!(
            &outcome,
            Err(super::super::agent_restart::RestartAttemptError::Request(
                super::super::agent_restart::AgentRestartError::BackgroundWork { .. }
            ))
        ) && crate::service::background_work::running(service, tab).is_empty()
        {
            schedule_async(service, Some(tab.into()));
        }
        return;
    }
    let (restart, label) = match outcome {
        Ok(outcome) if outcome.session_action() == "resumed" => (RestartState::Resumed, "resumed"),
        Ok(_) => (RestartState::FreshLaunch, "fresh_launch"),
        Err(error) => {
            log::error!("[provider-update] tab {tab} restart failed: {error:?}");
            (RestartState::Failed, "restart_failed")
        }
    };
    super::journal(
        provider,
        target,
        Some(tab),
        label,
        "provider_update_restart",
    );
    if let Err(error) = storage::transact(|state| state.settle_restart(key, tab, restart)) {
        log::error!("[provider-update] restart completion persistence failed: {error}");
    }
    refresh_summary(service, key);
}

pub(super) fn refresh_summary(service: &ServiceContext, key: &str) {
    let Ok(Some(run)) = storage::transact(|state| {
        if state.claim_notice(key) {
            state.runs.get(key).cloned()
        } else {
            None
        }
    }) else {
        return refresh_existing_summary(service, key);
    };
    let (title, body, failed) = summary(service, &run);
    let id = super::notify_owner(service, key, title, body, failed);
    let _ = storage::transact(|state| {
        if let Some(run) = state.runs.get_mut(key) {
            run.summary_id = Some(id);
        }
    });
    refresh_existing_summary(service, key);
}

fn refresh_existing_summary(service: &ServiceContext, key: &str) {
    let Ok(state) = storage::read() else {
        return;
    };
    let Some(run) = state.runs.get(key) else {
        return;
    };
    let Some(id) = run.summary_id.as_deref() else {
        return;
    };
    let (title, body, _) = summary(service, run);
    let updated = service.notif.lock().update_summary(id, title, body);
    if let Some(payload) = updated {
        service.notify_event("notification-updated", payload);
    }
}

fn summary(service: &ServiceContext, run: &UpdateRun) -> (String, String, bool) {
    let display = service
        .agent_registry
        .provider_arc(&run.provider)
        .map(|provider| provider.display_name().to_string())
        .unwrap_or_else(|| run.provider.clone());
    let title = format!("{display} {} -> {}", run.current, run.target);
    match &run.outcome {
        RunOutcome::Failed(error) | RunOutcome::UnstoppedFailure(error) => {
            (format!("{title}: update failed"), error.clone(), true)
        }
        _ => {
            let (restarted, pending) = run.counts();
            let failed = run
                .tabs
                .values()
                .filter(|state| **state == RestartState::Failed)
                .count();
            (title, format!("{restarted} idle tabs restarted, {pending} pending, {failed} restart failures."), false)
        }
    }
}
