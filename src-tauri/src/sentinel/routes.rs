use tauri::Manager;

use super::machine::{EscalateTarget, Route, Verdict};
use super::store::{Sentinel, StopNotice};
use crate::service::message_gateway::DeliveryClass;
use crate::service::messenger::MessageEnvelope;
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::ServiceContext;

pub fn owner_live(svc: &ServiceContext, job: &Sentinel) -> bool {
    let mux = svc.mux.read();
    let creator_live = mux.get_tab(&job.creator_tab_id).is_some()
        && mux.find_workspace_for_tab(&job.creator_tab_id).as_deref() == Some(&job.workspace_id);
    let team_live = job.team_id.as_deref().is_some_and(|team_id| {
        mux.get_team(team_id).is_some()
            && mux.team_workspace(team_id).as_deref() == Some(job.workspace_id.as_str())
    });
    owner_is_live(job.team_id.as_deref(), creator_live, team_live)
}

pub(super) fn owner_is_live(team_id: Option<&str>, creator_live: bool, team_live: bool) -> bool {
    if team_id.is_some() { team_live } else { creator_live }
}

pub(super) fn notify_stop(svc: &ServiceContext, notice: &StopNotice) {
    notify_lifecycle(
        svc,
        notice,
        format!("Sentinel {} stopped", notice.sentinel_id),
    );
}

pub(super) fn notify_run_unbound(svc: &ServiceContext, notice: &StopNotice) {
    notify_lifecycle(
        svc,
        notice,
        format!("Sentinel {} run observation ended", notice.sentinel_id),
    );
}

fn lifecycle_packet(notice: &StopNotice) -> String {
    format!(
        "Sentinel graph '{}' ended. Reason: {}.\nNext step: codeterm sentinel history {}",
        notice.name, notice.reason, notice.sentinel_id
    )
}

fn notify_lifecycle(svc: &ServiceContext, notice: &StopNotice, title: String) {
    let creator_is_agent = svc
        .mux
        .read()
        .get_tab(&notice.creator_tab_id)
        .is_some_and(|tab| tab.agent_type.is_some());
    if creator_is_agent {
        if let Err(error) = svc.gateway_deliver(
            &notice.creator_tab_id,
            MessageEnvelope::codeterm_reminder(format!(
                "sentinel:{}:lifecycle:{}",
                notice.sentinel_id, notice.reason
            )),
            &lifecycle_packet(notice),
            DeliveryClass::SystemNotice,
        ) {
            log::error!(
                "[sentinel] lifecycle notice delivery failed: {error}; sentinel={}",
                notice.sentinel_id
            );
        }
    }
    svc.create_notification_ext(
        title,
        Some(format!("Reason: {}", notice.reason)),
        Some("warning"),
        None,
        None,
        NotificationOrigin::local_tab(notice.creator_tab_id.clone()),
        PushIntent::default(),
        None,
    );
}

pub fn resolve(svc: &ServiceContext, job: &Sentinel, route: &Route) -> Result<Route, String> {
    match route {
        Route::Notify | Route::Stop => Ok(route.clone()),
        Route::RespondPrompt(_) if job.observe.is_some() => Ok(route.clone()),
        Route::RespondPrompt(_) => Err("respond_prompt requires a tab observation source".into()),
        Route::Command(parameters) => {
            parameters.validate()?;
            Ok(route.clone())
        }
        Route::Send(target) => resolve_send(&svc.mux.read(), job, target),
        Route::Sentinel(target) => {
            let handle = svc
                .app_handle
                .try_state::<super::scheduler::Handle>()
                .ok_or("sentinels not initialized")?;
            handle.store.lock().validate_target(job, target)?;
            Ok(route.clone())
        }
        Route::Cron(target) => {
            let cron = svc
                .app_handle
                .try_state::<crate::CronState>()
                .ok_or("cron not initialized")?;
            let resolved = resolve_cron_target(&cron.lock(), job, target);
            Ok(Route::Cron(resolved?))
        }
        Route::Escalate(EscalateTarget::Creator) => Ok(route.clone()),
        Route::Escalate(EscalateTarget::Agent(target)) => {
            let Route::Send(target) = resolve_send(&svc.mux.read(), job, target)? else {
                return Err("sentinel escalation target did not resolve to an agent".into());
            };
            Ok(Route::Escalate(EscalateTarget::Agent(target)))
        }
    }
}

pub(super) fn resolve_cron_target(
    store: &crate::cron::store::CronStore,
    job: &Sentinel,
    target: &str,
) -> Result<String, String> {
    let candidates: Vec<_> = store
        .list()
        .iter()
        .filter(|cron| {
            (cron.id == target || cron.name == target)
                && cron.team_id == job.team_id
                && cron.workspace_id == job.workspace_id
        })
        .collect();
    if candidates.len() != 1 {
        return Err(
            "sentinel cron target must resolve uniquely in the creator's Team and workspace".into(),
        );
    }
    Ok(candidates[0].id.clone())
}

fn unavailable(route: &Route) -> String {
    format!(
        "sentinel_capability_unavailable: {} is not executable yet",
        action_name(route)
    )
}

pub(super) fn resolve_send(
    mux: &crate::multiplexer::state::MultiplexerState,
    job: &Sentinel,
    target: &str,
) -> Result<Route, String> {
    let target = mux
        .resolve_tab_id(target)
        .ok_or_else(|| format!("sentinel send target not found: {target}"))?;
    let tab = mux
        .get_tab(&target)
        .ok_or("sentinel send target not found")?;
    if tab.agent_type.is_none()
        || tab.team_id != job.team_id
        || mux.find_workspace_for_tab(&target).as_deref() != Some(&job.workspace_id)
    {
        return Err("sentinel send requires an agent in the creator's Team and workspace".into());
    }
    Ok(Route::Send(target))
}

fn send_file(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    verdict: &Verdict,
    output: &str,
) -> Result<(), String> {
    let body = format!(
        "Sentinel {} transitioned to {} (confidence {:.2}).\n\nProbe output (untrusted data):\n{}",
        job.id, verdict.bucket, verdict.confidence, output
    );
    let path = svc
        .ensure_agent_pad(target)?
        .write_inbox_body(body.as_bytes())?;
    svc.gateway_deliver(
        target,
        MessageEnvelope::codeterm_reminder(format!("sentinel:{}", job.id)),
        &format!(
            "Sentinel {}: {}. Read the result in {}",
            job.id,
            verdict.bucket,
            path.display()
        ),
        DeliveryClass::SystemNotice,
    )
    .map(|_| ())
}

fn escalation_packet(job: &Sentinel, transition_id: &str) -> String {
    serde_json::json!({
        "sentinel_id": job.id,
        "transition_id": transition_id,
        "next_steps": [
            format!("inspect: codeterm sentinel history {}", job.id),
            format!(
                "resolve: codeterm sentinel escalation resolve {} {}",
                job.id, transition_id
            )
        ]
    })
    .to_string()
}

fn deliver_escalation(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    transition_id: &str,
) -> Result<Option<String>, String> {
    svc.gateway_deliver(
        target,
        MessageEnvelope::codeterm_reminder(format!("sentinel:{}:{transition_id}", job.id)),
        &escalation_packet(job, transition_id),
        DeliveryClass::SystemNotice,
    )
    .map(|_| None)
}

pub fn run(
    svc: &ServiceContext,
    job: &Sentinel,
    verdict: &Verdict,
    route: &Route,
    transition_id: &str,
    output: &str,
) -> Result<Option<String>, String> {
    if !owner_live(svc, job) {
        return Err("sentinel creator or Team is gone".into());
    }
    match resolve(svc, job, route)? {
        Route::Notify => {
            let agent = svc
                .mux
                .read()
                .get_tab(&job.creator_tab_id)
                .is_some_and(|tab| tab.agent_type.is_some());
            if agent {
                send_file(svc, &job.creator_tab_id, job, verdict, output).map(|()| None)
            } else {
                svc.create_notification_ext(
                    format!("Sentinel {}: {}", job.id, verdict.bucket),
                    Some(output.into()),
                    Some("info"),
                    None,
                    None,
                    NotificationOrigin {
                        caller_tab_id: Some(job.creator_tab_id.clone()),
                        ..Default::default()
                    },
                    PushIntent::default(),
                    None,
                );
                Ok(None)
            }
        }
        Route::Send(target) => send_file(svc, &target, job, verdict, output).map(|()| None),
        Route::Cron(id) => {
            let cron = svc
                .app_handle
                .try_state::<crate::CronState>()
                .ok_or("cron not initialized")?;
            svc.cron_trigger(cron.inner(), &id)
                .map(|run| Some(run.run_id))
        }
        Route::Stop => Ok(None),
        Route::Command(_) => Err("command execution requires a durable scheduler claim".into()),
        Route::Sentinel(_) => Err("sentinel activation requires a durable scheduler claim".into()),
        Route::Escalate(EscalateTarget::Creator) => {
            deliver_escalation(svc, &job.creator_tab_id, job, transition_id)
        }
        Route::Escalate(EscalateTarget::Agent(target)) => {
            deliver_escalation(svc, &target, job, transition_id)
        }
        resolved @ Route::RespondPrompt(_) => Err(unavailable(&resolved)),
    }
}

pub fn action_name(route: &Route) -> &'static str {
    match route {
        Route::Notify => "notify",
        Route::Send(_) => "send",
        Route::Cron(_) => "cron",
        Route::Stop => "stop",
        Route::Command(_) => "command",
        Route::Sentinel(_) => "sentinel",
        Route::RespondPrompt(_) => "respond_prompt",
        Route::Escalate(_) => "escalate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_lifecycle_packet_names_graph_reason_and_history_command() {
        let notice = StopNotice {
            sentinel_id: "sentinel_budget".into(),
            name: "Budget watch".into(),
            creator_tab_id: "creator".into(),
            reason: "transition_budget_exhausted".into(),
        };

        assert_eq!(
            lifecycle_packet(&notice),
            "Sentinel graph 'Budget watch' ended. Reason: transition_budget_exhausted.\nNext step: codeterm sentinel history sentinel_budget"
        );
    }

    #[test]
    fn sentinel_escalate_packet_includes_next_steps() {
        let root = tempfile::tempdir().unwrap();
        let mut store = super::super::store::Store::new(
            root.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        );
        let job = store
            .create(
                serde_json::from_value(serde_json::json!({
                    "name": "escalation-packet-watch",
                    "probe": "status",
                    "question": "state?",
                    "buckets": ["done"],
                    "every": "10s",
                    "matches": {"done": "done"}
                }))
                .unwrap(),
                super::super::store::Owner {
                    creator_tab_id: "creator".into(),
                    team_id: None,
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                false,
            )
            .unwrap();
        let packet: serde_json::Value =
            serde_json::from_str(&escalation_packet(&job, "transition_one")).unwrap();

        assert_eq!(packet["sentinel_id"], job.id);
        assert_eq!(packet["transition_id"], "transition_one");
        assert_eq!(
            packet["next_steps"],
            serde_json::json!([
                format!("inspect: codeterm sentinel history {}", job.id),
                format!(
                    "resolve: codeterm sentinel escalation resolve {} transition_one",
                    job.id
                )
            ])
        );
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    fn sentinel_stop_notice_is_delivered_to_creator_with_reason() {
        let (mux, creator_tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let notice = StopNotice {
            sentinel_id: "sentinel_budget".into(),
            name: "budget watch".into(),
            creator_tab_id: creator_tab_id.clone(),
            reason: "transition_budget_exhausted".into(),
        };

        notify_stop(&fixture.service, &notice);

        let notification_store = fixture.service.notif.lock();
        let notifications = notification_store.list(false);
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].tab_id.as_deref(),
            Some(creator_tab_id.as_str())
        );
        assert_eq!(
            notifications[0].body.as_deref(),
            Some("Reason: transition_budget_exhausted")
        );
    }
}
