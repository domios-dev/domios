use super::*;
use codeterm_plugin::blueprint_step::*;
use codeterm_plugin::graph_nodes::{GraphNodeRegistry, RegisteredStep};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct PluginStepStoreRoot(pub std::path::PathBuf);

fn root(service: &ServiceContext) -> Result<std::path::PathBuf, String> {
    let path = service
        .host_extension::<PluginStepStoreRoot>()
        .map_or_else(codeterm_service_core::paths::data_dir, |root| {
            root.0.clone()
        });
    std::fs::create_dir_all(&path).map_err(|_| "plugin run store unavailable")?;
    Ok(path)
}

pub(crate) fn ledger(service: &ServiceContext) -> Result<OperationLedger, String> {
    OperationLedger::open(&root(service)?.join("blueprint-plugin-operations.sqlite"))
}

pub(crate) fn media(service: &ServiceContext) -> Result<RunMediaRegistry, String> {
    RunMediaRegistry::open(
        &root(service)?.join("blueprint-run-media.sqlite"),
        Arc::new(RunMediaFiles {
            registry: service
                .ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
                .clone(),
            runtime: service.rt.clone(),
        }),
    )
}

pub fn resolve_run_view(
    service: &ServiceContext,
    context: &codeterm_plugin::view_host::ViewCallContext,
    reference: &str,
) -> Result<serde_json::Value, String> {
    service.validate_plugin_view_context(context)?;
    let scope = context.owner_scope().ok_or("owned run view required")?;
    let view = ledger(service)?.resolve_run_view(reference)?;
    if view.scope.workspace != scope.workspace
        || view.scope.team != scope.team
        || view.owner != scope.owner
    {
        return Err("run view scope refused".into());
    }
    Ok(
        serde_json::json!({"workspaceId":view.scope.workspace,"target":{"blueprintId":view.blueprint,"runId":view.scope.run,"teamId":view.scope.team}}),
    )
}

pub(crate) fn operation(run: &str, node: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"blueprint-plugin-step-v1\0");
    hash.update((run.len() as u64).to_le_bytes());
    hash.update(run.as_bytes());
    hash.update(node.as_bytes());
    format!("{:x}", hash.finalize())
}

pub(super) fn run(
    ctx: &RunCtx<'_>,
    step: &Step,
    input: &ExecutionEnvelope,
    limit: Duration,
) -> Result<StepResult, JournalReason> {
    let StepKind::Plugin {
        node_type,
        config,
        params,
        ..
    } = &step.kind
    else {
        return Err(JournalReason::ContractViolation);
    };
    let registered = ctx.service.ext::<Arc<GraphNodeRegistry>>().step(node_type);
    let Some(registered) = registered else {
        return Ok(held("plugin_step_unavailable"));
    };
    let ledger = match ledger(ctx.service) {
        Ok(ledger) => ledger,
        Err(_) => return Ok(held("plugin_step_store_unavailable")),
    };
    let scope = RunScope {
        workspace: ctx.lease.job.workspace_id.clone(),
        team: ctx.lease.job.team_id.clone(),
        run: ctx.run_id.into(),
    };
    if ledger
        .admit_run(&scope, &config.spending.unit, config.spending.run_ceiling)
        .is_err()
    {
        return Ok(held("plugin_step_authorization_unavailable"));
    }
    let operation = operation(ctx.run_id, &step.name);
    let view = match ledger.run_view(
        &scope,
        &ctx.lease.job.creator_tab_id,
        &ctx.lease.job.id,
        ctx.lease
            .job
            .pipeline()
            .map(|spec| spec.name.as_str())
            .unwrap_or("Blueprint"),
    ) {
        Ok(view) => view,
        Err(_) => return Ok(held("plugin_step_store_unavailable")),
    };
    let existing = match ledger.get(&operation) {
        Ok(existing) => existing,
        Err(_) => return Ok(held("plugin_step_store_unavailable")),
    };
    if let Some(record) = &existing {
        if record.request.context.scope != scope
            || record.request.context.owner != ctx.lease.job.creator_tab_id
            || record.request.context.node != step.name
            || record.plugin != registered.plugin
        {
            return Err(JournalReason::ContractViolation);
        }
        if record.state.terminal() {
            return result(record.clone());
        }
        if matches!(
            record.state,
            OperationState::Queued
                | OperationState::Running
                | OperationState::Unknown
                | OperationState::Submitting
        ) {
            return result(record.clone());
        }
    }
    let inputs = match inputs(&registered, input) {
        Ok(inputs) => inputs,
        Err(_) => return Ok(held("plugin_step_input_unavailable")),
    };
    let media = match media(ctx.service) {
        Ok(media) => media,
        Err(_) => return Ok(held("plugin_step_media_unavailable")),
    };
    let mut request = StepRequest {
        context: StepContext {
            scope,
            owner: ctx.lease.job.creator_tab_id.clone(),
            node: step.name.clone(),
            attempt: uuid::Uuid::new_v4().simple().to_string(),
            operation,
            run_view: view.reference,
            run_label: view.label,
            node_label: label(&registered.descriptor.title, 128),
            input_lineage: Vec::new(),
        },
        node_type: node_type.clone(),
        revision: config.revision,
        params: params.clone(),
        inputs,
        unit: config.spending.unit.clone(),
        step_allowance: config.spending.step_allowance,
    };
    if let Some(existing) = existing {
        request.context = existing.request.context;
    }
    request.context.input_lineage.clear();
    for values in request.inputs.values() {
        for value in values {
            if let PortValue::Media {
                handle,
                media: kind,
            } = value
            {
                if media
                    .authorize_handoff(&request.context, handle, &registered.plugin, &step.name)
                    .is_err()
                {
                    return Ok(held("plugin_step_input_unavailable"));
                }
                let lineage = match media.input(&request.context, &registered.plugin, handle, *kind)
                {
                    Ok(lineage) => lineage,
                    Err(_) => return Ok(held("plugin_step_input_unavailable")),
                };
                if !request.context.input_lineage.contains(&lineage) {
                    request.context.input_lineage.push(lineage);
                }
            }
        }
    }
    let runtime = StepRuntime {
        ledger: &ledger,
        descriptor: &registered.descriptor,
        adapter: registered.adapter.clone(),
        media: Some(&media),
    };
    let record = runtime.drive(
        &registered.plugin,
        &request,
        || now(ctx.handle),
        Instant::now() + limit,
        ctx.lease.cancelled.conversion_token(),
    );
    match record {
        Ok(record) => result(record),
        Err(_) => Ok(held("plugin_step_held")),
    }
}

fn inputs(registered: &RegisteredStep, input: &ExecutionEnvelope) -> Result<PortValues, String> {
    let descriptor = &registered.descriptor;
    let values = if descriptor.inputs.is_empty() {
        Default::default()
    } else if let Ok(values) = serde_json::from_value::<PortValues>(input.value.clone()) {
        values
    } else if descriptor.inputs.len() == 1 {
        let (name, port) = descriptor
            .inputs
            .iter()
            .next()
            .ok_or("input port missing")?;
        let value = match port.kind {
            PortKind::Text if input.media == "text" => PortValue::Text {
                text: input.payload_text(),
            },
            PortKind::Json if input.media == "json" => PortValue::Json {
                value: input.value.clone(),
            },
            _ => return Err("plugin input requires typed port bindings".into()),
        };
        [(name.clone(), vec![value])].into()
    } else {
        return Err("plugin input requires typed port bindings".into());
    };
    descriptor.validate_values(&values, false)?;
    Ok(values)
}

fn result(record: OperationRecord) -> Result<StepResult, JournalReason> {
    match record.state {
        OperationState::Succeeded => {
            let output = ExecutionEnvelope {
                media: "json".into(),
                schema_id: None,
                value: serde_json::to_value(record.outputs)
                    .map_err(|_| JournalReason::ContractViolation)?,
                text: String::new(),
            };
            let bytes =
                serde_json::to_vec(&output).map_err(|_| JournalReason::ContractViolation)?;
            if bytes.len() > codeterm_blueprint_types::blueprint_execution::MAX_STDIN_BYTES {
                return Err(JournalReason::ContractViolation);
            }
            let outcome = record.outcome.ok_or(JournalReason::ContractViolation)?;
            Ok(StepResult {
                outcome: StepOutcome::Bucket(outcome.clone()),
                stored_outcome: outcome.into(),
                state: JournalState::Done,
                summary: "Plugin step generated its outputs".into(),
                reason: None,
                next_text: String::from_utf8(bytes)
                    .map_err(|_| JournalReason::ContractViolation)?,
                decision: None,
                uncertain: false,
            })
        }
        OperationState::Failed | OperationState::Canceled => Ok(StepResult {
            outcome: StepOutcome::Failed,
            stored_outcome: "failed".into(),
            state: JournalState::Failed,
            summary: "Plugin step ended without a result".into(),
            reason: Some("plugin_step_failed".into()),
            next_text: String::new(),
            decision: None,
            uncertain: false,
        }),
        OperationState::WaitingForOwnerOk => Ok(held("plugin_step_waiting_for_owner_ok")),
        OperationState::Queued => Ok(held("plugin_step_queued")),
        OperationState::Running => Ok(held("plugin_step_running")),
        OperationState::Submitting => Ok(held("plugin_step_submitting")),
        _ => Ok(held("plugin_step_unknown")),
    }
}

fn held(reason: &str) -> StepResult {
    StepResult {
        outcome: StepOutcome::Failed,
        stored_outcome: "held".into(),
        state: JournalState::Held,
        summary: "Plugin step is waiting".into(),
        reason: Some(reason.to_string().into()),
        next_text: String::new(),
        decision: None,
        uncertain: false,
    }
}

fn label(text: &str, maximum: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(maximum / 4)
        .collect()
}
