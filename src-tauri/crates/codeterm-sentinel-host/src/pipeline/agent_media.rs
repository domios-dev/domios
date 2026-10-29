use super::*;
use codeterm_plugin::agent_task::{AgentTaskMedia, AgentTaskMediaKind};
use codeterm_plugin::blueprint_step::*;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) fn inputs(
    ctx: &RunCtx<'_>,
    step: &Step,
    input: &ExecutionEnvelope,
) -> Result<Vec<AgentTaskMedia>, JournalReason> {
    let StepKind::Agent { provider, .. } = &step.kind else {
        return Ok(Vec::new());
    };
    let Ok(values) = serde_json::from_value::<PortValues>(input.value.clone()) else {
        return Ok(Vec::new());
    };
    if !scope_live(ctx) {
        return Err(JournalReason::ContractViolation);
    }
    let registry =
        super::plugin::media(ctx.service).map_err(|_| JournalReason::JournalWriteFailed)?;
    let files = RunMediaFiles {
        registry: ctx
            .service
            .ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
            .clone(),
        runtime: ctx.service.rt.clone(),
    };
    let context = StepContext {
        scope: RunScope {
            workspace: ctx.lease.job.workspace_id.clone(),
            team: ctx.lease.job.team_id.clone(),
            run: ctx.run_id.into(),
        },
        owner: ctx.lease.job.creator_tab_id.clone(),
        node: step.name.clone(),
        operation: super::plugin::operation(ctx.run_id, &step.name),
        attempt: ctx.run_id.into(),
        run_view: ctx.run_id.into(),
        run_label: String::new(),
        node_label: String::new(),
        input_lineage: Vec::new(),
    };
    let mut media = Vec::new();
    for value in values.values().flatten() {
        if matches!(value, PortValue::MediaSource { .. }) {
            return Err(JournalReason::ContractViolation);
        }
        let PortValue::Media {
            handle,
            media: kind,
        } = value
        else {
            continue;
        };
        if media.len() == 4 || ctx.lease.cancelled.load(Ordering::SeqCst) {
            return Err(JournalReason::ContractViolation);
        }
        let modality = match kind {
            PortKind::Image => AgentTaskMediaKind::Image,
            PortKind::Audio => AgentTaskMediaKind::Audio,
            PortKind::Video => AgentTaskMediaKind::Video,
            _ => return Err(JournalReason::ContractViolation),
        };
        let backend = ctx
            .service
            .chatbackend_registry
            .get(provider)
            .ok_or(JournalReason::RuntimeUnavailable)?;
        if !backend.agent_task_media().contains(&modality) {
            return Err(JournalReason::ContractViolation);
        }
        registry
            .authorize_handoff(&context, handle, provider, &step.name)
            .map_err(|_| JournalReason::ContractViolation)?;
        let lineage = registry
            .input(&context, provider, handle, *kind)
            .map_err(|_| JournalReason::ContractViolation)?;
        let source = registry
            .resolve_input(&context, provider, handle, *kind)
            .map_err(|_| JournalReason::ContractViolation)?;
        let mut opened = files
            .open(
                &source.lookup,
                &source.media_id,
                source.kind,
                Some(&source.fingerprint),
            )
            .map_err(|_| JournalReason::ContractViolation)?;
        if opened.size > 2 * 1024 * 1024 {
            return Err(JournalReason::BudgetExhausted);
        }
        let mut bytes = Vec::with_capacity(opened.size as usize);
        (&mut opened.file)
            .take(opened.size)
            .read_to_end(&mut bytes)
            .map_err(|_| JournalReason::ContractViolation)?;
        if bytes.len() as u64 != opened.size
            || format!("{:x}", Sha256::digest(&bytes)) != source.fingerprint
        {
            return Err(JournalReason::ContractViolation);
        }
        media.push(AgentTaskMedia {
            kind: modality,
            content_type: opened.content_type,
            file_name: opened.file_name,
            bytes,
            sha256: source.fingerprint,
            lineage,
        });
    }
    if !scope_live(ctx) {
        return Err(JournalReason::ContractViolation);
    }
    Ok(media)
}

fn scope_live(ctx: &RunCtx<'_>) -> bool {
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

pub(super) fn review_verdict(value: &serde_json::Value) -> Result<&str, String> {
    let object = value
        .as_object()
        .ok_or("agent review must return a typed verdict")?;
    let verdict = object
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "pass" | "retry" | "choose"))
        .ok_or("agent review verdict unavailable")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "verdict" | "notes" | "choice"))
        || object.get("notes").is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|text| text.len() > 4096 || text.contains('\0'))
        })
        || object
            .get("choice")
            .is_some_and(|value| value.as_u64().is_none_or(|index| index >= 256))
        || (verdict == "choose" && !object.contains_key("choice"))
        || (verdict != "choose" && object.contains_key("choice"))
    {
        return Err("invalid bounded agent review verdict".into());
    }
    Ok(verdict)
}

#[cfg(test)]
mod tests;
