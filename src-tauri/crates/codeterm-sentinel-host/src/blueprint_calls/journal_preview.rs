use codeterm_blueprint_types::blueprint_document::calls::{
    projection::{bounded, bounded_text},
    CallFrame,
};
use codeterm_sentinel::blueprint_snapshot::JournalPreview;
use codeterm_storage::BlueprintJournalEntry;
use std::collections::BTreeSet;

pub(super) fn project(entry: &BlueprintJournalEntry) -> JournalPreview {
    let mut native_keys = BTreeSet::from([
        "entry_id",
        "run_id",
        "step_index",
        "step_name",
        "state",
        "outcome",
        "started_at_ms",
        "ended_at_ms",
        "result_summary",
        "reason",
        "body_ref",
    ]);
    if !entry.call_stack.is_empty() {
        native_keys.insert("call_stack");
    }
    if entry.parent_step_index.is_some() {
        native_keys.insert("parent_step_index");
    }
    if entry.definition_version.is_some() {
        native_keys.insert("definition_version");
    }
    let keys: BTreeSet<_> = entry
        .evidence
        .keys()
        .map(String::as_str)
        .chain(native_keys.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(128)
        .collect();
    JournalPreview {
        evidence: entry
            .evidence
            .iter()
            .filter(|(key, _)| keys.contains(key.as_str()) && !native_keys.contains(key.as_str()))
            .map(|(key, value)| (key.clone(), bounded(value, 1)))
            .collect(),
        entry_id: keys.contains("entry_id").then(|| entry.entry_id),
        run_id: keys.contains("run_id").then(|| bounded_text(&entry.run_id)),
        step_index: keys.contains("step_index").then(|| entry.step_index),
        step_name: keys
            .contains("step_name")
            .then(|| bounded_text(&entry.step_name)),
        state: keys.contains("state").then(|| entry.state),
        outcome: keys.contains("outcome").then(|| {
            entry
                .outcome
                .as_ref()
                .map(|value| bounded_text(value.as_str()))
        }),
        started_at_ms: keys.contains("started_at_ms").then(|| entry.started_at_ms),
        ended_at_ms: keys.contains("ended_at_ms").then(|| entry.ended_at_ms),
        result_summary: keys
            .contains("result_summary")
            .then(|| entry.result_summary.as_deref().map(bounded_text)),
        reason: keys.contains("reason").then(|| {
            entry
                .reason
                .as_ref()
                .map(|value| bounded_text(value.as_str()))
        }),
        body_ref: keys
            .contains("body_ref")
            .then(|| entry.body_ref.as_deref().map(bounded_text)),
        call_stack: (!entry.call_stack.is_empty() && keys.contains("call_stack")).then(|| {
            entry
                .call_stack
                .iter()
                .take(128)
                .map(|frame| CallFrame {
                    node: bounded_text(&frame.node),
                    id: bounded_text(&frame.id),
                    hash: bounded_text(&frame.hash),
                })
                .collect()
        }),
        parent_step_index: entry
            .parent_step_index
            .filter(|_| keys.contains("parent_step_index")),
        definition_version: entry
            .definition_version
            .filter(|_| keys.contains("definition_version")),
    }
}

#[cfg(test)]
mod tests;
