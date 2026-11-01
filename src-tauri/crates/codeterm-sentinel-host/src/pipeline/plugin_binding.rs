use super::*;
use codeterm_blueprint_types::blueprint_document::bindings::Binding;
use codeterm_plugin::{blueprint_step::*, graph_nodes::GraphNodeRegistry};
use serde_json::Value;
use std::sync::Arc;

pub(super) fn resolve(
    ctx: &RunCtx<'_>,
    binding: &Binding<'_>,
    consumer: Option<&Step>,
) -> Result<Option<ExecutionEnvelope>, JournalReason> {
    let Some(original) = ctx.outputs.get(binding.node) else {
        return Err(JournalReason::ContractViolation);
    };
    if serde_json::from_value::<PortValues>(original.value.clone()).is_err() {
        return Ok(None);
    }
    let ledger = plugin::ledger(ctx.service).map_err(|_| JournalReason::ContractViolation)?;
    let Some(record) = ledger
        .get(&plugin::operation(ctx.run_id, binding.node))
        .map_err(|_| JournalReason::ContractViolation)?
    else {
        return Ok(None);
    };
    if record.state != OperationState::Succeeded
        || record.request.context.scope.workspace != ctx.lease.job.workspace_id
        || record.request.context.scope.team != ctx.lease.job.team_id
        || record.request.context.owner != ctx.lease.job.creator_tab_id
        || record.request.context.scope.run != ctx.run_id
        || record.request.context.node != binding.node
    {
        return Err(JournalReason::ContractViolation);
    }
    let registered = ctx
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .step(&record.request.node_type)
        .ok_or(JournalReason::ContractViolation)?;
    if registered.plugin != record.plugin
        || registered.descriptor.revision != record.request.revision
    {
        return Err(JournalReason::ContractViolation);
    }
    let port = registered
        .descriptor
        .outputs
        .get(binding.selection)
        .ok_or(JournalReason::ContractViolation)?;
    if original.value
        != serde_json::to_value(&record.outputs).map_err(|_| JournalReason::ContractViolation)?
    {
        return Err(JournalReason::ContractViolation);
    }
    let values = record
        .outputs
        .get(binding.selection)
        .cloned()
        .unwrap_or_default();
    let mut plain = Vec::with_capacity(values.len());
    for value in values {
        plain.push(match value {
            PortValue::Text { text } => Value::String(text),
            PortValue::Json { value } => value,
            PortValue::Media {
                handle,
                media: kind,
            } => {
                if !live(ctx) || ctx.remaining().is_zero() {
                    return Err(JournalReason::Deadline);
                }
                let mut context = record.request.context.clone();
                context.node = consumer.map_or("result", |step| step.name.as_str()).into();
                let media =
                    plugin::media(ctx.service).map_err(|_| JournalReason::ContractViolation)?;
                media
                    .authorize_handoff(&context, &handle, "blueprint", &context.node)
                    .map_err(|_| JournalReason::ContractViolation)?;
                let source = media
                    .resolve_input(&context, "blueprint", &handle, kind)
                    .map_err(|_| JournalReason::ContractViolation)?;
                let opened = RunMediaFiles {
                    registry: ctx
                        .service
                        .ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
                        .clone(),
                    runtime: ctx.service.rt.clone(),
                }
                .open(
                    &source.lookup,
                    &source.media_id,
                    kind,
                    Some(&source.fingerprint),
                )
                .map_err(|_| JournalReason::ContractViolation)?;
                if !live(ctx) {
                    return Err(JournalReason::ContractViolation);
                }
                Value::String(
                    opened
                        .path
                        .to_str()
                        .ok_or(JournalReason::ContractViolation)?
                        .into(),
                )
            }
            PortValue::MediaSource { .. } => return Err(JournalReason::ContractViolation),
        });
    }
    let value = if port.max_items == 1 {
        plain.pop().unwrap_or(Value::Null)
    } else {
        Value::Array(plain)
    };
    Ok(Some(ExecutionEnvelope {
        media: if port.kind != PortKind::Json && port.max_items == 1 {
            "text"
        } else {
            "json"
        }
        .into(),
        schema_id: None,
        value,
        text: String::new(),
    }))
}

fn live(ctx: &RunCtx<'_>) -> bool {
    let mux = ctx.service.mux.read();
    !ctx.lease.cancelled.load(Ordering::SeqCst)
        && mux
            .get_tab(&ctx.lease.job.creator_tab_id)
            .is_some_and(|owner| owner.team_id == ctx.lease.job.team_id)
        && mux
            .find_workspace_for_tab(&ctx.lease.job.creator_tab_id)
            .as_deref()
            == Some(ctx.lease.job.workspace_id.as_str())
}
