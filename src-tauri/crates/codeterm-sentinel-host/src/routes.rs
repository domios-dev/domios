mod pipeline_result;
#[cfg(test)]
pub(super) use pipeline_result::deliver_pipeline_result_with;
pub(super) use pipeline_result::{
    deliver_pipeline_result, notify_pipeline_result, pipeline_result_owner,
    write_pipeline_result_body, PipelineResultDelivery,
};

use super::machine::{EscalateTarget, Route, Verdict};
use super::store::{Sentinel, StopNotice};
use codeterm_service_core::service::message_gateway::DeliveryClass;
use codeterm_service_core::service::messenger::MessageEnvelope;
use codeterm_service_core::service::ServiceContext;

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
    if team_id.is_some() {
        team_live
    } else {
        creator_live
    }
}

pub(super) fn notify_stop(svc: &ServiceContext, notice: &StopNotice) {
    notify_lifecycle(
        svc,
        notice,
        format!("Blueprint {} stopped", notice.sentinel_id),
    );
}

pub(super) fn notify_run_unbound(svc: &ServiceContext, notice: &StopNotice) {
    notify_lifecycle(
        svc,
        notice,
        format!("Blueprint {} run observation ended", notice.sentinel_id),
    );
}

pub(super) fn notify_trigger_unavailable(svc: &ServiceContext, job: &Sentinel, reason: &str) {
    let recipient = {
        let mux = svc.mux.read();
        if let Some(team_id) = job.team_id.as_deref() {
            mux.all_tabs()
                .find(|tab| {
                    tab.agent_type.is_some()
                        && tab.anchors_team()
                        && tab.team_id.as_deref() == Some(team_id)
                        && mux.find_workspace_for_tab(&tab.id).as_deref()
                            == Some(job.workspace_id.as_str())
                })
                .map(|tab| (tab.id.clone(), true))
        } else {
            mux.get_tab(&job.creator_tab_id)
                .map(|tab| (tab.id.clone(), tab.agent_type.is_some()))
        }
    };
    let Some((recipient, is_agent)) = recipient else {
        return;
    };
    let reason = reason.chars().take(256).collect::<String>();
    let title = format!("Blueprint {} trigger unavailable", job.id);
    let message = format!(
        "Blueprint '{}' cannot receive events: {reason}. Next step: inspect domios blueprint history {}",
        job.name.as_deref().unwrap_or(&job.id),
        job.id
    );
    if is_agent {
        if let Err(error) = svc.gateway_deliver(
            &recipient,
            MessageEnvelope::codeterm_reminder(format!("blueprint:{}:unavailable", job.id)),
            &message,
            DeliveryClass::SystemNotice,
        ) {
            log::error!(
                "[blueprint] unavailable notice delivery failed: {error}; blueprint={}",
                job.id
            );
        }
    } else {
        svc.log_system_notice(title, Some(message), Some("warning"));
    }
}

fn lifecycle_packet(notice: &StopNotice) -> String {
    format!(
        "Blueprint graph '{}' ended. Reason: {}.\nNext step: domios blueprint history {}",
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
                "blueprint:{}:lifecycle:{}",
                notice.sentinel_id, notice.reason
            )),
            &lifecycle_packet(notice),
            DeliveryClass::SystemNotice,
        ) {
            log::error!(
                "[blueprint] lifecycle notice delivery failed: {error}; blueprint={}",
                notice.sentinel_id
            );
        }
    }
    svc.log_system_notice(
        title,
        Some(format!("Reason: {}", notice.reason)),
        Some("warning"),
    );
}

pub fn resolve(svc: &ServiceContext, job: &Sentinel, route: &Route) -> Result<Route, String> {
    match route {
        Route::Notify | Route::Stop => Ok(route.clone()),
        Route::RespondPrompt(_) if job.observe().is_some() => Ok(route.clone()),
        Route::RespondPrompt(_) => Err("respond_prompt requires a tab observation source".into()),
        Route::Command(parameters) => {
            parameters.validate()?;
            Ok(route.clone())
        }
        Route::Send(target) => resolve_send(&svc.mux.read(), job, target),
        Route::Sentinel(target) => {
            let handle = svc
                .host_extension::<super::scheduler::Handle>()
                .ok_or("blueprints not initialized")?;
            handle.store.lock().validate_target(job, target)?;
            Ok(route.clone())
        }
        Route::Cron(target) => {
            let cron = svc
                .try_ext::<codeterm_service_core::CronState>()
                .ok_or("cron not initialized")?;
            let resolved = resolve_cron_target(&cron.lock(), job, target);
            Ok(Route::Cron(resolved?))
        }
        Route::Escalate(EscalateTarget::Creator) => Ok(route.clone()),
        Route::Escalate(EscalateTarget::Agent(target)) => {
            let Route::Send(target) = resolve_send(&svc.mux.read(), job, target)? else {
                return Err("blueprint escalation target did not resolve to an agent".into());
            };
            if !svc
                .mux
                .read()
                .get_tab(&target)
                .is_some_and(|tab| tab.agent_type.is_some())
            {
                return Err("blueprint escalation requires an agent target".into());
            }
            Ok(Route::Escalate(EscalateTarget::Agent(target)))
        }
    }
}

pub(super) fn resolve_cron_target(
    store: &codeterm_service_core::cron::store::CronStore,
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
            "blueprint cron target must resolve uniquely in the creator's Team and workspace"
                .into(),
        );
    }
    Ok(candidates[0].id.clone())
}

fn unavailable(route: &Route) -> String {
    format!(
        "blueprint_capability_unavailable: {} is not executable yet",
        action_name(route)
    )
}

pub(super) fn resolve_send(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    job: &Sentinel,
    target: &str,
) -> Result<Route, String> {
    let target = mux
        .resolve_tab_id(target)
        .ok_or_else(|| format!("blueprint send target not found: {target}"))?;
    let tab = mux
        .get_tab(&target)
        .ok_or("blueprint send target not found")?;
    if tab.team_id != job.team_id
        || mux.find_workspace_for_tab(&target).as_deref() != Some(&job.workspace_id)
    {
        return Err("blueprint send requires a tab in the creator's Team and workspace".into());
    }
    Ok(Route::Send(target))
}

pub(super) fn deliver_body(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    output: &str,
    run_id: &str,
) -> Result<(), String> {
    let agent = svc
        .mux
        .read()
        .get_tab(target)
        .ok_or("send target disappeared")?
        .agent_type
        .is_some();
    deliver_body_with(
        agent,
        output,
        |body| svc.send_text(target, &svc.programmatic_input(target, body)),
        |body| {
            let mut envelope = MessageEnvelope::codeterm_reminder(format!("blueprint:{}", job.id));
            let delivery = svc.durable_delivery(run_id, &job.creator_tab_id);
            envelope.durable_delivery = Some(delivery);
            svc.gateway_deliver(target, envelope, body, DeliveryClass::SystemNotice)
                .map(|_| ())
        },
    )
}

pub(super) fn deliver_body_with(
    agent: bool,
    body: &str,
    shell: impl FnOnce(&str) -> Result<(), String>,
    routed: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    if agent {
        routed(body)
    } else {
        shell(body)
    }
}

fn send_file(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    verdict: &Verdict,
    output: &str,
    transition_id: &str,
) -> Result<(), String> {
    let delivery = svc.durable_delivery(transition_id, &job.creator_tab_id);
    let body = format!(
        "Blueprint {} transitioned to {} (confidence {:.2}).\n\nProbe output (untrusted data):\n{}",
        job.id, verdict.bucket, verdict.confidence, output
    );
    let path = svc
        .ensure_agent_pad(target)?
        .write_inbox_body(body.as_bytes())?;
    let mut envelope = MessageEnvelope::codeterm_reminder(format!("blueprint:{}", job.id));
    envelope.durable_delivery = Some(delivery.clone());
    envelope.id = Some(format!(
        "{transition_id}:{}",
        path.file_name()
            .ok_or("durable result file name missing")?
            .to_string_lossy()
    ));
    svc.gateway_deliver(
        target,
        envelope,
        &format!(
            "Blueprint {}: {}. Read the result in {}",
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
        "blueprint_id": job.id,
        "transition_id": transition_id,
        "next_steps": [
            format!("inspect: domios blueprint history {}", job.id),
            format!(
                "resolve: domios blueprint escalation resolve {} {}",
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
        MessageEnvelope::codeterm_reminder(format!("blueprint:{}:{transition_id}", job.id)),
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
        return Err("Blueprint owner or Team is gone".into());
    }
    codeterm_service_core::flow_trace::record(
        transition_id,
        "blueprint.started",
        &job.creator_tab_id,
        "",
        "route",
        None,
    );
    match resolve(svc, job, route)? {
        Route::Notify => {
            let agent = svc
                .mux
                .read()
                .get_tab(&job.creator_tab_id)
                .is_some_and(|tab| tab.agent_type.is_some());
            if agent {
                send_file(
                    svc,
                    &job.creator_tab_id,
                    job,
                    verdict,
                    output,
                    transition_id,
                )
                .map(|()| None)
            } else {
                svc.log_system_notice(
                    format!("Blueprint {}: {}", job.id, verdict.bucket),
                    Some(output.into()),
                    Some("info"),
                );
                Ok(None)
            }
        }
        Route::Send(target) => {
            deliver_body(svc, &target, job, output, transition_id).map(|()| None)
        }
        Route::Cron(id) => {
            let cron = svc
                .try_ext::<codeterm_service_core::CronState>()
                .ok_or("cron not initialized")?;
            svc.cron_trigger(cron, &id).map(|run| {
                codeterm_service_core::flow_trace::record(
                    transition_id,
                    "blueprint.cron",
                    &job.creator_tab_id,
                    "",
                    "triggered",
                    Some(&run.run_id),
                );
                Some(run.run_id)
            })
        }
        Route::Stop => Ok(None),
        Route::Command(_) => Err("command execution requires a durable scheduler claim".into()),
        Route::Sentinel(_) => Err("blueprint activation requires a durable scheduler claim".into()),
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
    route.name()
}

#[cfg(test)]
mod tests;
