use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub(super) struct Checkpoint {
    run_id: String,
    pub current_step: Option<usize>,
    pub failed: bool,
    pub envelope: ExecutionEnvelope,
    pub outputs: BTreeMap<String, ExecutionEnvelope>,
    pub next_marker: u16,
    text: String,
    input_source: Option<PathBuf>,
    input_kind: Option<MediaKind>,
    input_failure: Option<String>,
    input_display: String,
}

impl Checkpoint {
    pub fn prepared_input(&self) -> PreparedInput {
        PreparedInput {
            text: self.text.clone(),
            cleanup: self
                .input_source
                .as_ref()
                .and_then(|path| path.parent())
                .map(|path| std::sync::Arc::new(crate::input::Cleanup::new(path.to_path_buf()))),
            input_source: self.input_source.clone(),
            input_kind: self.input_kind,
            input_failure: self.input_failure.clone(),
            input_display: Some(self.input_display.clone()),
            input_run: None,
        }
    }
}

pub(super) fn load(job: &Sentinel) -> Result<Option<Checkpoint>, JournalReason> {
    job.admission
        .continuation
        .as_ref()
        .map(|saved| {
            let checkpoint: Checkpoint = serde_json::from_value(saved.clone())
                .map_err(|_| JournalReason::JournalWriteFailed)?;
            if checkpoint
                .current_step
                .is_some_and(|index| job.pipeline().is_none_or(|spec| index >= spec.steps.len()))
            {
                return Err(JournalReason::ContractViolation);
            }
            Ok(checkpoint)
        })
        .transpose()
}

pub(super) fn hold(
    ctx: &RunCtx<'_>,
    state: &RunnerState,
    envelope: &ExecutionEnvelope,
    text: &str,
    _input_run: Option<&InputRun>,
) -> Result<(), JournalReason> {
    persist(ctx, state, envelope, text, None)
}

pub(super) fn hold_plugin(
    ctx: &RunCtx<'_>,
    state: &RunnerState,
    envelope: &ExecutionEnvelope,
    text: &str,
    hold: codeterm_sentinel::admission::PluginHold,
) -> Result<(), JournalReason> {
    persist(ctx, state, envelope, text, Some(hold))
}

fn persist(
    ctx: &RunCtx<'_>,
    state: &RunnerState,
    envelope: &ExecutionEnvelope,
    text: &str,
    plugin: Option<codeterm_sentinel::admission::PluginHold>,
) -> Result<(), JournalReason> {
    let saved = serde_json::to_value(Checkpoint {
        run_id: ctx.run_id.into(),
        current_step: state.current_step,
        failed: state.status == RunStatus::Failed,
        envelope: envelope.clone(),
        outputs: ctx.outputs.clone(),
        next_marker: ctx.next_marker,
        text: text.into(),
        input_source: ctx.input_source.clone(),
        input_kind: ctx.input_kind,
        input_failure: ctx.input_failure.clone(),
        input_display: ctx.input_display.clone(),
    })
    .map_err(|_| JournalReason::JournalWriteFailed)?;
    if saved.to_string().len() > 4 * 1024 * 1024 {
        return Err(JournalReason::BudgetExhausted);
    }
    let committed = storage::commit(&ctx.handle.store, |store| {
        let Some(mut job) = store.get(&ctx.lease.job.id).cloned() else {
            return storage::Staged::ready(Ok(false));
        };
        if !job.same_owner(&ctx.lease.job)
            || job.definition_version != ctx.lease.job.definition_version
            || job.machine.stopped
        {
            return storage::Staged::ready(Ok(false));
        }
        if let Some(plugin) = plugin {
            job.admission.plugin_hold = Some(plugin);
        } else {
            job.admission.reconcile(true, false);
        }
        job.admission.continuation = Some(saved);
        store.stage_admission(&job.id, job.admission)
    })
    .map_err(|_| JournalReason::JournalWriteFailed)?;
    if !committed {
        return Err(JournalReason::JournalWriteFailed);
    }
    if let Some(cleanup) = &ctx.cleanup {
        cleanup.retain();
    }
    ctx.handle.notify();
    let held = {
        ctx.handle
            .store
            .lock()
            .get(&ctx.lease.job.id)
            .and_then(|job| job.admission.plugin_hold.clone())
    };
    if let Some(held) = held {
        if let Ok(ledger) = super::plugin::ledger(ctx.service) {
            if let Ok(Some(record)) = ledger.get(&held.operation) {
                let _ = ctx.service.invalidate_plugin_owner_views(
                    &codeterm_plugin::view_host::ViewOwnerScope {
                        plugin: record.plugin,
                        workspace: ctx.lease.job.workspace_id.clone(),
                        team: ctx.lease.job.team_id.clone(),
                        owner: ctx.lease.job.creator_tab_id.clone(),
                    },
                );
            }
        }
    }
    Ok(())
}

pub(super) fn clear(handle: &Handle, source: &Sentinel) {
    let result = storage::commit(&handle.store, |store| {
        let Some(mut job) = store.get(&source.id).cloned() else {
            return storage::Staged::ready(Ok(false));
        };
        if job.admission.continuation.take().is_none() {
            return storage::Staged::ready(Ok(false));
        }
        store.stage_admission(&job.id, job.admission)
    });
    if let Err(error) = result {
        log::error!("[blueprint] run checkpoint settlement failed: {error}");
    }
    handle.notify();
}

pub(crate) fn retained_inputs(handle: &Handle, data_dir: &Path) -> BTreeSet<PathBuf> {
    handle
        .store
        .lock()
        .list()
        .iter()
        .filter_map(|job| {
            let saved = load(job).ok()??;
            codeterm_blueprint_types::sentinel_input::run_dir(data_dir, &job.id, &saved.run_id).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests;
