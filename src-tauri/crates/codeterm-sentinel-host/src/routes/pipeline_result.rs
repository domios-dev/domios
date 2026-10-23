use super::*;
use codeterm_blueprint_types::blueprint_pipeline::{BlueprintOwner, BlueprintSpec};
use codeterm_events::{SeqEvent, SystemEvent};
use codeterm_service_core::service::messenger::Delivery;
use std::time::Duration;
use tokio::sync::broadcast;

pub(crate) struct PipelineResultDelivery {
    pub state: &'static str,
    pub reason: &'static str,
    pub body_ref: Option<String>,
}

pub(crate) fn pipeline_result_owner(
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

pub(crate) fn write_pipeline_result_body(
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

pub(crate) fn deliver_pipeline_result(
    svc: &ServiceContext,
    target: &str,
    job: &Sentinel,
    verdict: &Verdict,
    path: &std::path::Path,
    body_ref: String,
    run_id: &str,
) -> PipelineResultDelivery {
    let class = match result_delivery_class(svc, target) {
        Ok(class) => class,
        Err(reason) => return result_receipt("failed", reason, body_ref),
    };
    let mut events = svc.events.subscribe();
    let delivery = svc.durable_delivery(run_id, &job.creator_tab_id);
    let mut envelope = MessageEnvelope::codeterm_reminder(format!("blueprint:{}:result", job.id));
    envelope.durable_delivery = Some(delivery);
    let packet = format!(
        "Blueprint {}: {}. Read the durable result in {}",
        job.id,
        verdict.bucket,
        path.display()
    );
    deliver_pipeline_result_with(target, &packet, body_ref, |target, packet| {
        let receipt = svc.gateway_deliver(target, envelope, packet, class)?;
        codeterm_service_core::async_runtime::block_on(confirm_result_receipt(
            receipt,
            &mut events,
            target,
            Duration::from_secs(60),
            |id| runtime_receipt(svc, target, id),
        ))
    })
}

pub(crate) fn deliver_pipeline_result_with(
    target: &str,
    packet: &str,
    body_ref: String,
    deliver: impl FnOnce(&str, &str) -> Result<(), String>,
) -> PipelineResultDelivery {
    match deliver(target, packet) {
        Ok(()) => result_receipt("delivered", "delivery_confirmed", body_ref),
        Err(error) => result_receipt("failed", delivery_failure_reason(&error), body_ref),
    }
}

pub(crate) fn notify_pipeline_result(
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

fn result_receipt(
    state: &'static str,
    reason: &'static str,
    body_ref: String,
) -> PipelineResultDelivery {
    PipelineResultDelivery {
        state,
        reason,
        body_ref: Some(body_ref),
    }
}

fn delivery_failure_reason(error: &str) -> &'static str {
    match error {
        "result_delivery_deadline" => "result_delivery_deadline",
        "result_target_unavailable" => "result_target_unavailable",
        "result_delivery_evidence_lost" => "result_delivery_evidence_lost",
        _ if error.starts_with("pty_unavailable:") => "result_target_unavailable",
        _ if error.starts_with("tab_busy_refused:") => "result_target_refused",
        _ => "result_delivery_unconfirmed",
    }
}

fn result_delivery_class(
    svc: &ServiceContext,
    target: &str,
) -> Result<DeliveryClass, &'static str> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(target).ok_or("result_target_unavailable")?;
    if tab.boot_stage == Some(codeterm_types::pty::BootStage::SpawnFailed) {
        return Err("result_target_unavailable");
    }
    match &tab.backing {
        codeterm_service_core::multiplexer::types::TabBacking::Pty
            if tab.agent_type.is_none() && !tab.agent_booting =>
        {
            Ok(DeliveryClass::AgentTraffic)
        }
        codeterm_service_core::multiplexer::types::TabBacking::Pty
        | codeterm_service_core::multiplexer::types::TabBacking::ChatBackend { .. } => {
            Ok(DeliveryClass::SystemNotice)
        }
        _ => Err("result_target_unavailable"),
    }
}

fn runtime_receipt(svc: &ServiceContext, target: &str, id: &str) -> Option<Result<(), String>> {
    use codeterm_service_core::service::chat_send_transaction::ChatSendState;
    let status = svc.chat_send_ledger.status(id)?;
    if status.tab_id != target {
        return None;
    }
    match status.state {
        ChatSendState::Written | ChatSendState::Sent => Some(Ok(())),
        ChatSendState::Failed => Some(Err("result_target_unavailable".into())),
        _ => None,
    }
}

async fn confirm_result_receipt(
    receipt: Delivery,
    events: &mut broadcast::Receiver<SeqEvent>,
    target: &str,
    deadline: Duration,
    status: impl Fn(&str) -> Option<Result<(), String>>,
) -> Result<(), String> {
    match receipt {
        Delivery::Pending { delivery_id } | Delivery::Unconfirmed { delivery_id } => {
            await_result_delivery(events, target, &delivery_id, deadline, || {
                status(&delivery_id)
            })
            .await
        }
        Delivery::Pty { .. } | Delivery::ChatBackend { .. } => Ok(()),
    }
}

async fn await_result_delivery(
    events: &mut broadcast::Receiver<SeqEvent>,
    target: &str,
    id: &str,
    deadline: Duration,
    receipt: impl Fn() -> Option<Result<(), String>>,
) -> Result<(), String> {
    tokio::time::timeout(deadline, async {
        loop {
            if let Some(result) = receipt() {
                return result;
            }
            let event = events
                .recv()
                .await
                .map_err(|_| "result_delivery_evidence_lost".to_string())?;
            match event.event {
                SystemEvent::TabClosed { tab_id } if tab_id == target => {
                    return Err("result_target_unavailable".into())
                }
                SystemEvent::Generic { event_type, data }
                    if data["tab_id"].as_str() == Some(target)
                        && data["delivery_id"].as_str() == Some(id) =>
                {
                    if event_type == "delivery-written" {
                        return Ok(());
                    }
                    if event_type == "agent-delivery-status" && data["status"] == "failed" {
                        return Err("result_target_unavailable".into());
                    }
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| "result_delivery_deadline".to_string())?
}

#[cfg(test)]
mod tests;
