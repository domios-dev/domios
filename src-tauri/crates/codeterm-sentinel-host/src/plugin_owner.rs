use crate::{pipeline::plugin, scheduler::Handle, storage};
use codeterm_plugin::{blueprint_step::*, graph_nodes::GraphNodeRegistry};
use codeterm_service_core::{
    ports::daemon_question::DaemonQuestionHandler, service::ServiceContext,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

pub(crate) struct OwnerQuestions(pub Weak<ServiceContext>);

pub fn preview_from_view(
    service: &ServiceContext,
    context: &codeterm_plugin::view_host::ViewCallContext,
    operation: &str,
    decision_key: &str,
) -> Result<Value, String> {
    let record = view_record(service, context, operation)?;
    let handle = service
        .host_extension::<Handle>()
        .ok_or("Blueprint scheduler unavailable")?;
    let now = handle.store.lock().clock.now_ms();
    if !handle.store.lock().list().iter().any(|job| {
        !job.machine.stopped
            && job.admission.plugin_hold.as_ref().is_some_and(|held| {
                held.operation == operation
                    && held.run == record.request.context.scope.run
                    && cost_hold(held)
            })
    }) {
        return Err("plugin operation is not awaiting price approval".into());
    }
    if owner_decision_key(&record) != decision_key
        || record.state != OperationState::WaitingForOwnerOk
        || !fresh(&record, now)
    {
        return Err("fresh bounded quote required".into());
    }
    let quote = record.estimate.as_ref().ok_or("quote unavailable")?;
    let registered = service
        .ext::<Arc<GraphNodeRegistry>>()
        .step(&record.request.node_type)
        .ok_or("plugin step unavailable")?;
    if registered.plugin != record.plugin
        || registered.descriptor.revision != record.request.revision
    {
        return Err("plugin step changed".into());
    }
    let amount_display = registered.cost_units.format(quote.amount, &quote.unit)?;
    Ok(
        json!({"operation":operation,"decisionKey":decision_key,"runLabel":record.request.context.run_label,
        "nodeLabel":record.request.context.node_label,"amount":quote.amount,"unit":quote.unit,
        "amountDisplay":amount_display,"aboveRunBudget":record.waiting.as_ref().is_some_and(|waiting| above_run_budget(quote, waiting))}),
    )
}

fn above_run_budget(quote: &Estimate, waiting: &Waiting) -> bool {
    quote.unit == waiting.unit && quote.amount > waiting.remaining
}

#[cfg(test)]
mod tests;

fn view_record(
    service: &ServiceContext,
    context: &codeterm_plugin::view_host::ViewCallContext,
    operation: &str,
) -> Result<OperationRecord, String> {
    service.validate_plugin_view_context(context)?;
    let scope = context
        .owner_scope()
        .ok_or("owned operation view required")?;
    let record = plugin::ledger(service)?
        .get(operation)?
        .ok_or("plugin operation unavailable")?;
    if record.plugin != context.plugin
        || record.request.context.owner != scope.owner
        || record.request.context.scope.workspace != scope.workspace
        || record.request.context.scope.team != scope.team
    {
        return Err("plugin operation owner scope refused".into());
    }
    Ok(record)
}

fn cost_hold(held: &codeterm_sentinel::admission::PluginHold) -> bool {
    held.reason == "plugin_step_waiting_for_owner_ok"
}

pub fn decide_from_view(
    service: &ServiceContext,
    context: &codeterm_plugin::view_host::ViewCallContext,
    operation: &str,
    decision_key: &str,
    action: &str,
) -> Result<Value, String> {
    let record = view_record(service, context, operation)?;
    let label = match action {
        "approve" => approve_label(service, &record).ok_or("fresh bounded quote required")?,
        "check_price" => "Check price again".into(),
        "check" => "Check again".into(),
        "cancel" => "Cancel this step".into(),
        _ => return Err("plugin operation decision unavailable".into()),
    };
    OwnerQuestions(
        service
            .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
            .clone(),
    )
    .answer_operation(decision_key, operation, &label)
}

fn question(record: &OperationRecord) -> String {
    owner_decision_key(record)
}

fn fresh(record: &OperationRecord, now: u64) -> bool {
    record.estimate.as_ref().is_some_and(|quote| {
        quote.valid_until_ms > now
            && quote.valid_until_ms <= MAX_COST_AMOUNT
            && quote.amount <= MAX_COST_AMOUNT
            && quote.unit == record.request.unit
    })
}

fn approve_label(service: &ServiceContext, record: &OperationRecord) -> Option<String> {
    let quote = record.estimate.as_ref()?;
    let registered = service
        .ext::<Arc<GraphNodeRegistry>>()
        .step(&record.request.node_type)?;
    let amount = registered
        .cost_units
        .format(quote.amount, &quote.unit)
        .ok()?;
    Some(format!("Approve {amount} for this step"))
}

pub(crate) fn publish(service: &ServiceContext, handle: &Handle) -> Result<(), String> {
    let jobs = handle.store.lock().list().to_vec();
    let now = handle.store.lock().clock.now_ms();
    let ledger = plugin::ledger(service)?;
    for job in jobs {
        if job.machine.stopped {
            continue;
        }
        let Some(held) = &job.admission.plugin_hold else {
            continue;
        };
        let Some(record) = ledger.get(&held.operation)? else {
            continue;
        };
        if !matches!(
            record.state,
            OperationState::WaitingForOwnerOk | OperationState::Unknown
        ) {
            continue;
        }
        let waiting = record.state == OperationState::WaitingForOwnerOk;
        let mut choices = vec![
            if waiting {
                "Check price again".into()
            } else {
                "Check again".into()
            },
            "Cancel this step".into(),
        ];
        if waiting && cost_hold(held) && fresh(&record, now) {
            if let Some(label) = approve_label(service, &record) {
                choices.insert(0, label);
            }
        }
        service.publish_daemon_question(question(&record),
            format!("{}: {}", record.request.context.run_label, if waiting { "waiting for your OK" } else { "plugin job is waiting" }),
            format!("{} is held. Approval applies only to this operation and never raises the run ceiling for later steps.", record.request.context.node_label),
            choices)?;
    }
    Ok(())
}

impl DaemonQuestionHandler for OwnerQuestions {
    fn answer(&self, request: &str, label: &str) -> Option<Result<Value, String>> {
        let operation = request
            .strip_prefix("blueprint-plugin:")?
            .split_once(':')?
            .0;
        Some(self.answer_operation(request, operation, label))
    }
}

impl OwnerQuestions {
    pub(crate) fn answer_operation(
        &self,
        question_id: &str,
        operation: &str,
        label: &str,
    ) -> Result<Value, String> {
        let service = self.0.upgrade().ok_or("Blueprint service unavailable")?;
        let handle = service
            .host_extension::<Handle>()
            .ok_or("Blueprint scheduler unavailable")?;
        let ledger = plugin::ledger(&service)?;
        let record = ledger
            .get(operation)?
            .ok_or("plugin operation unavailable")?;
        if question(&record) != question_id {
            return Err("plugin quote changed; open the current owner question".into());
        }
        let job = handle
            .store
            .lock()
            .list()
            .iter()
            .find(|job| {
                job.admission.plugin_hold.as_ref().is_some_and(|held| {
                    held.operation == operation && held.run == record.request.context.scope.run
                })
            })
            .cloned()
            .ok_or("plugin operation is no longer held")?;
        {
            let mux = service.mux.read();
            let owner = mux
                .get_tab(&record.request.context.owner)
                .ok_or("plugin operation owner unavailable")?;
            if mux.find_workspace_for_tab(&owner.id).as_deref()
                != Some(record.request.context.scope.workspace.as_str())
                || owner.team_id != record.request.context.scope.team
                || job.creator_tab_id != record.request.context.owner
                || job.workspace_id != record.request.context.scope.workspace
                || job.team_id != record.request.context.scope.team
                || job.machine.stopped
            {
                return Err("plugin operation owner scope changed".into());
            }
        }
        let registered = service
            .ext::<Arc<GraphNodeRegistry>>()
            .step(&record.request.node_type)
            .ok_or("plugin step unavailable")?;
        if registered.plugin != record.plugin
            || registered.descriptor.revision != record.request.revision
        {
            return Err("plugin step registration changed".into());
        }
        let media = plugin::media(&service)?;
        let runtime = StepRuntime {
            ledger: &ledger,
            descriptor: &registered.descriptor,
            adapter: registered.adapter.clone(),
            media: Some(&media),
        };
        let now = handle.store.lock().clock.now_ms();
        let refresh = label == "Check price again";
        let check = label == "Check again";
        if refresh {
            runtime.refresh_estimate(
                &record.request,
                || now,
                Instant::now() + Duration::from_secs(5),
                Default::default(),
            )?;
        } else if check {
            runtime.reconcile(
                &record.request,
                Instant::now() + Duration::from_secs(30),
                Default::default(),
            )?;
            ledger.advance_owner_question(
                operation,
                &record.request.context.scope,
                &record.request.context.owner,
            )?;
        } else if label == "Cancel this step" {
            runtime.cancel(&record.request, Instant::now() + Duration::from_secs(5))?;
        } else if job.admission.plugin_hold.as_ref().is_some_and(cost_hold)
            && approve_label(&service, &record).as_deref() == Some(label)
            && fresh(&record, now)
        {
            ledger.authorize_quoted(
                operation,
                &record.request.context.scope,
                &record.request.context.owner,
                record.estimate.as_ref().ok_or("quote unavailable")?.amount,
                now,
                question_id,
            )?;
        } else {
            return Err("fresh bounded owner authorization required".into());
        }
        let current = ledger
            .get(operation)?
            .ok_or("plugin operation unavailable")?;
        if refresh {
            storage::commit(&handle.store, |store| {
                let Some(mut current) = store.get(&job.id).cloned() else {
                    return storage::Staged::ready(Err("Blueprint removed".into()));
                };
                if current.machine.stopped || current.definition_version != job.definition_version {
                    return storage::Staged::ready(Err("Blueprint hold changed".into()));
                }
                let Some(held) = current
                    .admission
                    .plugin_hold
                    .as_mut()
                    .filter(|held| held.operation == operation)
                else {
                    return storage::Staged::ready(Err("Blueprint hold changed".into()));
                };
                held.reason = "plugin_step_waiting_for_owner_ok".into();
                held.state = "waiting_for_owner_ok".into();
                store.stage_admission(&job.id, current.admission)
            })?;
        }
        let resume = !refresh && (!check || current.state.terminal());
        if resume {
            storage::commit(&handle.store, |store| {
                let Some(mut current) = store.get(&job.id).cloned() else {
                    return storage::Staged::ready(Err("Blueprint removed".into()));
                };
                if current.machine.stopped
                    || current.definition_version != job.definition_version
                    || current
                        .admission
                        .plugin_hold
                        .as_ref()
                        .is_none_or(|held| held.operation != operation)
                {
                    return storage::Staged::ready(Err("Blueprint hold changed".into()));
                }
                current.admission.plugin_hold = None;
                store.stage_admission(&job.id, current.admission)
            })?;
        }
        handle.notify();
        let _ =
            service.invalidate_plugin_owner_views(&codeterm_plugin::view_host::ViewOwnerScope {
                plugin: record.plugin.clone(),
                workspace: record.request.context.scope.workspace.clone(),
                team: record.request.context.scope.team.clone(),
                owner: record.request.context.owner.clone(),
            });
        Ok(json!({"operation":operation,"recorded":true,"resuming":resume}))
    }
}
