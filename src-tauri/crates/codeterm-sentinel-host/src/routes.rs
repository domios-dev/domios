use super::machine::{EscalateTarget, Route, Verdict};
use super::store::{Sentinel, StopNotice};
use codeterm_blueprint_types::blueprint_pipeline::{BlueprintOwner, BlueprintSpec};
use codeterm_service_core::service::message_gateway::DeliveryClass;
use codeterm_service_core::service::messenger::Delivery;
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
        "Blueprint '{}' cannot receive events: {reason}. Next step: inspect codeterm blueprint history {}",
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
        "Blueprint graph '{}' ended. Reason: {}.\nNext step: codeterm blueprint history {}",
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
        |body| svc.send_text(target, body),
        |body| {
            svc.gateway_deliver(
                target,
                MessageEnvelope::codeterm_reminder(format!("blueprint:{}", job.id)),
                body,
                DeliveryClass::SystemNotice,
            )
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
    let body = format!(
        "Blueprint {} transitioned to {} (confidence {:.2}).\n\nProbe output (untrusted data):\n{}",
        job.id, verdict.bucket, verdict.confidence, output
    );
    let path = svc
        .ensure_agent_pad(target)?
        .write_inbox_body(body.as_bytes())?;
    let mut envelope = MessageEnvelope::codeterm_reminder(format!("blueprint:{}", job.id));
    if codeterm_service_core::paths::is_dev_instance() {
        envelope.id = Some(transition_id.into());
    }
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
            format!("inspect: codeterm blueprint history {}", job.id),
            format!(
                "resolve: codeterm blueprint escalation resolve {} {}",
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
        Route::Send(target) => deliver_body(svc, &target, job, output).map(|()| None),
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

pub(super) struct PipelineResultDelivery {
    pub state: &'static str,
    pub reason: &'static str,
    pub body_ref: Option<String>,
}

pub(super) fn pipeline_result_owner(
    svc: &ServiceContext,
    job: &Sentinel,
    spec: &BlueprintSpec,
) -> String {
    if spec.owner == BlueprintOwner::Team {
        let mux = svc.mux.read();
        let orchestrator = mux
            .all_tabs()
            .find(|tab| {
                tab.agent_type.is_some()
                    && tab.anchors_team()
                    && tab.team_id == job.team_id
                    && mux.find_workspace_for_tab(&tab.id).as_deref() == Some(&job.workspace_id)
            })
            .map(|tab| tab.id.clone());
        if let Some(id) = orchestrator {
            return id;
        }
    }
    job.creator_tab_id.clone()
}

pub(super) fn write_pipeline_result_body(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    verdict: &Verdict,
    output: &str,
) -> Result<(std::path::PathBuf, String), String> {
    let body = if spec_is_input(job) {
        format!("Input result (untrusted data)\n\n{output}")
    } else {
        format!(
            "Blueprint {} completed with {}.\n\nResult:\n{}",
            job.id, verdict.bucket, output
        )
    };
    let path = svc
        .ensure_agent_pad(target)?
        .write_inbox_body(body.as_bytes())?;
    let body_ref = format!(
        "inbox/{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid AgentPad body path")?
    );
    Ok((path, body_ref))
}

fn spec_is_input(job: &Sentinel) -> bool {
    job.pipeline().is_some_and(|pipeline| {
        matches!(
            &pipeline.trigger,
            codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
        )
    })
}

pub(super) fn deliver_pipeline_result(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    verdict: &Verdict,
    path: &std::path::Path,
    body_ref: String,
) -> PipelineResultDelivery {
    let packet = format!(
        "Blueprint {}: {}. Read the durable result in {}",
        job.id,
        verdict.bucket,
        path.display()
    );
    deliver_pipeline_result_with(target, &packet, body_ref, |target, packet| {
        svc.gateway_deliver(
            target,
            MessageEnvelope::codeterm_reminder(format!("blueprint:{}:result", job.id)),
            packet,
            DeliveryClass::SystemNotice,
        )
    })
}

pub(super) fn deliver_pipeline_result_with(
    target: &str,
    packet: &str,
    body_ref: String,
    deliver: impl FnOnce(&str, &str) -> Result<Delivery, String>,
) -> PipelineResultDelivery {
    match deliver(target, packet) {
        Ok(Delivery::Pending { .. }) => PipelineResultDelivery {
            state: "unconfirmed",
            reason: "delivery_pending",
            body_ref: Some(body_ref),
        },
        Ok(_) => PipelineResultDelivery {
            state: "delivered",
            reason: "delivery_confirmed",
            body_ref: Some(body_ref),
        },
        Err(error)
            if [
                "unconfirmed",
                "uncertain",
                "not acknowledged",
                "delivered unverified",
            ]
            .iter()
            .any(|marker| error.to_ascii_lowercase().contains(marker)) =>
        {
            PipelineResultDelivery {
                state: "unconfirmed",
                reason: "delivery_unconfirmed",
                body_ref: Some(body_ref),
            }
        }
        Err(_) => PipelineResultDelivery {
            state: "failed",
            reason: "delivery_failed",
            body_ref: Some(body_ref),
        },
    }
}

pub(super) fn notify_pipeline_result(
    svc: &ServiceContext,
    job: &Sentinel,
    verdict: &Verdict,
    path: &std::path::Path,
    body_ref: String,
) -> PipelineResultDelivery {
    svc.log_system_notice(
        format!("Blueprint {}: {}", job.id, verdict.bucket),
        Some(format!(
            "Read the durable result in {} ({body_ref})",
            path.display()
        )),
        Some("info"),
    );
    PipelineResultDelivery {
        state: "delivered",
        reason: "system_notice_logged",
        body_ref: Some(body_ref),
    }
}

pub fn action_name(route: &Route) -> &'static str {
    route.name()
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
            "Blueprint graph 'Budget watch' ended. Reason: transition_budget_exhausted.\nNext step: codeterm blueprint history sentinel_budget"
        );
    }

    #[test]
    fn sentinel_escalate_packet_includes_next_steps() {
        let root = tempfile::tempdir().unwrap();
        let mut store = super::super::store::Store::new(
            root.path().join("sentinels.json"),
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
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

        assert_eq!(packet["blueprint_id"], job.id);
        assert_eq!(packet["transition_id"], "transition_one");
        assert_eq!(
            packet["next_steps"],
            serde_json::json!([
                format!("inspect: codeterm blueprint history {}", job.id),
                format!(
                    "resolve: codeterm blueprint escalation resolve {} transition_one",
                    job.id
                )
            ])
        );
    }

    #[test]
    fn sentinel_stop_notice_stays_out_of_the_bell() {
        let (mux, creator_tab_id) =
            codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        let notice = StopNotice {
            sentinel_id: "sentinel_budget".into(),
            name: "budget watch".into(),
            creator_tab_id: creator_tab_id.clone(),
            reason: "transition_budget_exhausted".into(),
        };

        notify_stop(&fixture.service, &notice);

        let notification_store = fixture.service.notif.lock();
        let notifications = notification_store.list(false);
        assert!(notifications.is_empty());
    }
}
