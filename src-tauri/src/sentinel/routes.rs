use tauri::Manager;

use super::machine::{EscalateTarget, Route, Verdict};
use super::store::Sentinel;
use crate::service::message_gateway::DeliveryClass;
use crate::service::messenger::MessageEnvelope;
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::ServiceContext;

pub fn creator_live(svc: &ServiceContext, job: &Sentinel) -> bool {
    let mux = svc.mux.read();
    mux.get_tab(&job.creator_tab_id)
        .is_some_and(|tab| tab.team_id == job.team_id)
        && mux.find_workspace_for_tab(&job.creator_tab_id).as_deref() == Some(&job.workspace_id)
        && job
            .team_id
            .as_ref()
            .is_none_or(|id| mux.get_team(id).is_some())
}

pub fn resolve(svc: &ServiceContext, job: &Sentinel, route: &Route) -> Result<Route, String> {
    match route {
        Route::Notify | Route::Stop => Ok(route.clone()),
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
            Ok(Route::Cron(resolve_cron_target(&cron.lock(), job, target)?))
        }
        Route::Escalate(EscalateTarget::Creator) => Ok(route.clone()),
        Route::Escalate(EscalateTarget::Agent(target)) => {
            let Route::Send(target) = resolve_send(&svc.mux.read(), job, target)? else {
                return Err("sentinel escalation target did not resolve to an agent".into());
            };
            Ok(Route::Escalate(EscalateTarget::Agent(target)))
        }
        Route::RespondPrompt(_) => Err(unavailable(route)),
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
        "transition_id": transition_id
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
    if !creator_live(svc, job) {
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
    fn sentinel_escalate_packet_contains_only_stage_and_transition_ids() {
        let root = tempfile::tempdir().unwrap();
        let mut store = super::super::store::Store::new(
            root.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        );
        let job = store
            .create(
                serde_json::from_value(serde_json::json!({
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

        assert_eq!(packet.as_object().unwrap().len(), 2);
        assert_eq!(packet["sentinel_id"], job.id);
        assert_eq!(packet["transition_id"], "transition_one");
    }
}
