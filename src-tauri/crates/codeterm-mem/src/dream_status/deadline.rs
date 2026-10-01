//! Host-side finalization of a dreaming run its deadline cut short. The diary
//! keeps the log written so far, loses its WIP placeholders, and the run
//! records a partial finalization with reason "deadline". Nothing here relies
//! on the agent: the run is already over when this executes.

use super::{
    diary_evidence, is_placeholder_text, is_unfilled_value, log_field, log_fields, tagged_block,
    DiaryFinalization,
};
use crate::store::MemUpdate;
use crate::workers::GraphWorkerRegistry;
use crate::MemManager;

pub const DEADLINE_REASON: &str = "deadline";
/// Log field the host appends; its presence marks the diary as cut short.
pub const DEADLINE_FIELD: &str = "DEADLINE";
pub const DREAM_DIARY_TAG: &str = "dream-diary";
pub const DREAMS_AREA: &str = "_dreams";
/// Replaces a pending stage value, so the field stays named but reads as not reached.
pub const CUT_SHORT_VALUE: &str = "cut_short";

#[derive(Debug, Clone, PartialEq)]
pub struct DeadlineOutcome {
    pub diary_id: Option<String>,
    /// The diary body still held placeholders and was rewritten.
    pub diary_rewritten: bool,
    /// The run's finalization was pending and now records the deadline.
    pub finalization_recorded: bool,
}

/// The last log field that holds a real value, i.e. the last stage the run
/// logged before it was cut short.
pub fn last_logged_stage(body: &str) -> Option<String> {
    log_fields(body)
        .into_iter()
        .filter(|(name, value)| name != DEADLINE_FIELD && !is_unfilled_value(value))
        .map(|(name, _)| name)
        .last()
}

/// Deterministic insight for a cut-short diary, naming the last logged stage.
pub fn deadline_insight(last_stage: Option<&str>) -> String {
    match last_stage {
        Some(stage) => format!(
            "Cut short at the deadline after stage {stage}. The log below is what this run \
             finished; the next run picks up the rest."
        ),
        None => "Cut short at the deadline before any stage was logged; the next run starts \
                 over."
            .to_string(),
    }
}

/// The finalized body for a diary that still holds placeholders, or `None`
/// when the diary is already complete and must be left untouched.
pub fn deadline_body(body: &str, detail: &str) -> Option<String> {
    if diary_evidence("", body).is_complete() {
        return None;
    }
    let insight = deadline_insight(last_logged_stage(body).as_deref());
    let mut log: Vec<String> = tagged_block(body, "log")
        .unwrap_or_default()
        .lines()
        .map(cut_short_line)
        .filter(|line| !line.trim().is_empty())
        .collect();
    log.push(format!("{DEADLINE_FIELD}: {DEADLINE_REASON} ({detail})"));
    let log = format!("\n{}\n", log.join("\n"));

    let mut out = match tagged_block(body, "insight") {
        Some(existing) if is_placeholder_text(&existing) => {
            replace_block(body, "insight", &format!("\n{insight}\n"))
        }
        Some(_) => body.to_string(),
        None if body.trim().is_empty() => format!("<insight>\n{insight}\n</insight>"),
        None => body.to_string(),
    };
    out = if tagged_block(&out, "log").is_some() {
        replace_block(&out, "log", &log)
    } else {
        format!("{out}\n\n<log>{log}</log>")
    };
    Some(out)
}

fn cut_short_line(line: &str) -> String {
    match log_field(line) {
        Some((name, value)) if is_unfilled_value(&value) => format!("{name}: {CUT_SHORT_VALUE}"),
        _ => line.to_string(),
    }
}

fn replace_block(body: &str, tag: &str, content: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = body.find(&open).map(|i| i + open.len()) else {
        return body.to_string();
    };
    let Some(end) = body[start..].find(&close).map(|i| i + start) else {
        return body.to_string();
    };
    format!("{}{}{}", &body[..start], content, &body[end..])
}

/// Finalize the diary of a dreaming run that hit its hard deadline. The diary
/// is the one the run bound, else the newest dream diary created since the run
/// spawned (one dreaming run is active at a time). Idempotent: a complete diary
/// is never rewritten, and a finalization the agent already recorded is kept.
pub fn finalize_at_deadline(
    mem: &MemManager,
    workers: &GraphWorkerRegistry,
    worker_id: &str,
    detail: &str,
) -> Result<DeadlineOutcome, String> {
    let worker = workers
        .get(worker_id)
        .ok_or_else(|| format!("worker not found: {worker_id}"))?;
    let diary_id = match worker.diary_id.clone() {
        Some(id) => Some(id),
        None => mem
            .dream_diaries_since(worker.spawned_at)?
            .into_iter()
            .next(),
    };

    let mut diary_rewritten = false;
    if let Some(id) = diary_id.as_deref() {
        if let Some(item) = mem.get(id)? {
            if let Some(body) = deadline_body(&item.body, detail) {
                let summary_is_placeholder =
                    item.summary.as_deref().is_none_or(is_placeholder_text);
                let patch = MemUpdate {
                    summary: summary_is_placeholder
                        .then(|| deadline_insight(last_logged_stage(&item.body).as_deref())),
                    area: Some(DREAMS_AREA.to_string()),
                    body: Some(body),
                    ..MemUpdate::default()
                };
                diary_rewritten = mem.update(id, patch)?;
            }
        }
    }

    let finalization_recorded = workers.record_finalization_if_pending(
        worker_id,
        diary_id.clone(),
        DiaryFinalization::Partial {
            diary_id: diary_id.clone(),
            reason: DEADLINE_REASON.to_string(),
        },
    )?;
    Ok(DeadlineOutcome {
        diary_id,
        diary_rewritten,
        finalization_recorded,
    })
}
