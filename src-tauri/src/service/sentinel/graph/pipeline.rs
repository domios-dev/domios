use super::*;
use codeterm_types::blueprint_journal::{JournalMarker, JournalOutcome, JournalState};

pub(super) fn project_pipeline_contract(
    snapshot: &mut Snapshot,
    jobs: &[Sentinel],
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
    live_leases: &BTreeSet<String>,
    event_subscriptions: &codeterm_plugin::graph_events::SubscriptionTable,
) {
    let mut journal_revisions = jobs
        .iter()
        .filter(|job| job.pipeline().is_some())
        .filter_map(|job| {
            journals
                .get(&job.id)
                .and_then(|entries| entries.iter().max_by_key(|entry| entry.entry_id))
                .map(|entry| (job.id.as_str(), entry.entry_id))
        })
        .collect::<Vec<_>>();
    let mut event_revisions = jobs
        .iter()
        .filter(|job| {
            job.pipeline().is_some_and(|spec| {
                matches!(
                    &spec.trigger,
                    codeterm_types::blueprint_pipeline::Trigger::Event { .. }
                )
            })
        })
        .filter_map(|job| {
            event_subscriptions
                .summary(&job.id)
                .map(|summary| (job.id.as_str(), summary))
        })
        .collect::<Vec<_>>();
    if !journal_revisions.is_empty() || !event_revisions.is_empty() {
        journal_revisions.sort_unstable();
        event_revisions.sort_by_key(|(id, _)| *id);
        let mut digest = Sha256::new();
        digest.update(snapshot.revision.as_bytes());
        for (id, entry_id) in journal_revisions {
            digest.update(id.as_bytes());
            digest.update(entry_id.to_le_bytes());
        }
        for (id, summary) in event_revisions {
            digest.update(id.as_bytes());
            digest.update(summary.state.as_bytes());
            digest.update(summary.delivered.to_le_bytes());
            digest.update(summary.refused.to_le_bytes());
            digest.update(summary.last_event_at_ms.unwrap_or_default().to_le_bytes());
            if let Some(reason) = summary.reason {
                digest.update(reason.as_bytes());
            }
        }
        snapshot.revision = format!("{:x}", digest.finalize());
    }
    snapshot.blueprints.clear();
    for job in jobs.iter().filter(|job| job.is_scenario_root()) {
        let Some(node) = snapshot.nodes.iter_mut().find(|node| {
            node.pointer("/identity/id").and_then(Value::as_str) == Some(job.id.as_str())
                && node.pointer("/identity/kind").and_then(Value::as_str) == Some("blueprint")
        }) else {
            continue;
        };
        let Some(spec) = job.pipeline() else {
            continue;
        };
        let mut entries = journals
            .get(&job.id)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.entry_id));
        let mut seen = BTreeSet::new();
        let runs = entries
            .iter()
            .filter(|entry| seen.insert(entry.run_id.as_str()))
            .map(|entry| {
                let rows = entries
                    .iter()
                    .copied()
                    .filter(|row| row.run_id == entry.run_id)
                    .collect::<Vec<_>>();
                pipeline_run_view(job, spec, &rows)
            })
            .collect::<Vec<_>>();
        let latest = runs
            .first()
            .cloned()
            .unwrap_or_else(|| pipeline_run_view(job, spec, &[]));
        let state = if job.machine.stopped {
            "stopped"
        } else if job.machine.paused
            || job.machine.held.is_some()
            || matches!(
                job.last_reason.as_deref(),
                Some(
                    "uncertain_pipeline_claim"
                        | "uncertain_pipeline_effect"
                        | "uncertain_pipeline_result"
                )
            )
        {
            "held"
        } else if latest["closed"] == true {
            if latest["state"] == "failed" {
                "failed"
            } else if latest["state"] == "held" {
                "held"
            } else {
                "idle"
            }
        } else if live_leases.contains(&job.id) {
            "running"
        } else {
            latest["state"].as_str().unwrap_or("idle")
        };
        let (owner_kind, owner_tab) = match spec.owner {
            codeterm_types::blueprint_pipeline::BlueprintOwner::Tab => {
                ("tab", Some(job.creator_tab_id.as_str()))
            }
            codeterm_types::blueprint_pipeline::BlueprintOwner::Team => ("team", None),
        };
        let mut blueprint = json!({
            "id":job.id,"name":spec.name,
            "owner":{"kind":owner_kind,"tab_id":owner_tab,"team_id":job.team_id},
            "state":state,"run_id":latest["id"],"current_step":latest["current_step"],
            "last_event_at_ms":latest["last_event_at_ms"],"trigger":trigger_view(&spec.trigger),
            "steps":latest["steps"],"result":latest["result"],"runs":runs
        });
        if matches!(
            &spec.trigger,
            codeterm_types::blueprint_pipeline::Trigger::Event { .. }
        ) {
            let subscription = event_subscriptions.summary(&job.id);
            blueprint["event_subscription"] = json!({
                "state":subscription.as_ref().map(|item| item.state.as_str()).unwrap_or(if job.machine.stopped { "stopped" } else { "unavailable" }),
                "delivered":subscription.as_ref().map(|item| item.delivered).unwrap_or_default(),
                "refused":subscription.as_ref().map(|item| item.refused).unwrap_or_default(),
                "last_event_at_ms":subscription.as_ref().and_then(|item| item.last_event_at_ms),
                "reason":subscription.as_ref().and_then(|item| item.reason.clone()).or_else(|| (!job.machine.stopped).then(|| "not_subscribed".into()))
            });
        }
        node["state"] = json!(state);
        snapshot.blueprints.push(blueprint);
    }
}

fn pipeline_run_view(
    job: &Sentinel,
    spec: &codeterm_types::blueprint_pipeline::BlueprintSpec,
    entries: &[&BlueprintJournalEntry],
) -> Value {
    let mut seen = BTreeSet::new();
    let newest = entries
        .iter()
        .copied()
        .filter(|entry| seen.insert(entry.marker()))
        .collect::<Vec<_>>();
    let (closed_state, current) = BlueprintJournalEntry::derive_run(entries);
    let steps = spec
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let entry = newest
                .iter()
                .find(|entry| entry.marker() == JournalMarker::Step(index as u16));
            let summary = entries.iter().find(|entry| {
                entry.marker() == JournalMarker::Step(index as u16)
                    && entry.result_summary.is_some()
            });
            pipeline_step_view(index, &step.name, step_kind(&step.kind), entry, summary)
        })
        .collect::<Vec<_>>();
    let result = newest
        .iter()
        .find(|entry| entry.marker() == JournalMarker::Result);
    let result_state = match result.and_then(|entry| entry.outcome.as_ref()) {
        Some(JournalOutcome::HandlerResult) => "delivered",
        Some(JournalOutcome::HandlerUnconfirmed | JournalOutcome::HandlerFailed) => "failed",
        _ if result.is_some_and(|entry| entry.state == "running") => "delivering",
        _ => "waiting",
    };
    let state = match closed_state {
        Some(JournalState::Failed) => "failed",
        Some(JournalState::Held) => "held",
        Some(_) => "completed",
        None if newest
            .iter()
            .any(|entry| entry.state == JournalState::Running) =>
        {
            "running"
        }
        None if entries.is_empty() => "idle",
        None => "held",
    };
    json!({
        "id":entries.first().map(|entry| &entry.run_id),"state":state,"closed":closed_state.is_some(),
        "started_at_ms":entries.iter().map(|entry| entry.started_at_ms).min(),
        "last_event_at_ms":entries.iter().map(|entry| entry.ended_at_ms.unwrap_or(entry.started_at_ms)).max(),
        "current_step":current,"steps":steps,
        "result":{"target":result_target_view(&spec.result.to,spec.owner,job),"state":result_state,
            "reason":result.and_then(|entry| entry.reason.as_deref()),
            "body_ref":result.and_then(|entry| entry.body_ref.as_deref())}
    })
}

fn pipeline_step_view(
    index: usize,
    name: &str,
    kind: &str,
    entry: Option<&&BlueprintJournalEntry>,
    summary: Option<&&BlueprintJournalEntry>,
) -> Value {
    let state = entry.map_or("waiting", |entry| entry.state.as_str());
    json!({
        "index":index,
        "name":name,
        "kind":kind,
        "title":format!("{}: {name}", kind),
        "state":state,
        "outcome":entry.filter(|entry| !entry.outcome.as_ref().is_some_and(JournalOutcome::is_skip)).and_then(|entry| entry.outcome.clone()),
        "started_at_ms":entry.map(|entry| entry.started_at_ms),
        "ended_at_ms":entry.and_then(|entry| entry.ended_at_ms),
        "result_summary":summary.and_then(|entry| entry.result_summary.clone()),
        "reason":entry.and_then(|entry| entry.reason.clone())
    })
}

fn step_kind(kind: &codeterm_types::blueprint_pipeline::StepKind) -> &'static str {
    kind.name()
}

fn trigger_view(trigger: &codeterm_types::blueprint_pipeline::Trigger) -> Value {
    use codeterm_types::blueprint_pipeline::Trigger;
    match trigger {
        Trigger::Schedule { every } => {
            json!({"type":"schedule","summary":format!("every {every}")})
        }
        Trigger::Event { kind, .. } => {
            json!({"type":"event","summary":format!("event {kind}")})
        }
        Trigger::Input { kind, source } => {
            json!({"type":"input", "summary":input_summary(*kind, source)})
        }
    }
}

fn input_summary(
    kind: codeterm_types::sentinel_input::MediaKind,
    source: &codeterm_types::sentinel_input::DataRef,
) -> String {
    use codeterm_types::sentinel_input::DataRef;
    let (ref_type, display) = match source {
        DataRef::Text(_) => ("text", "inline text".into()),
        DataRef::File(path) => {
            let name = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            ("file", name.into_owned())
        }
        DataRef::Url(url) => (
            "url",
            url.split_once("://")
                .and_then(|(_, rest)| {
                    rest.split(|character| matches!(character, '/' | '?' | '#'))
                        .next()
                })
                .map(|authority| authority.rsplit('@').next().unwrap_or("source"))
                .unwrap_or("source")
                .to_string(),
        ),
    };
    format!("{} {ref_type} {display}", kind.as_str())
}

fn result_target_view(
    target: &codeterm_types::blueprint_pipeline::ResultTarget,
    owner: codeterm_types::blueprint_pipeline::BlueprintOwner,
    job: &Sentinel,
) -> Value {
    use codeterm_types::blueprint_pipeline::{BlueprintOwner, ResultTarget};
    match target {
        ResultTarget::Owner if owner == BlueprintOwner::Team => {
            json!({"kind":"team","id":job.team_id})
        }
        ResultTarget::Owner => json!({"kind":"owner_tab","id":job.creator_tab_id}),
        ResultTarget::Tab(id) => json!({"kind":"tab","id":id}),
        ResultTarget::Team => json!({"kind":"team","id":job.team_id}),
        ResultTarget::Notify => json!({"kind":"notify","id":Value::Null}),
    }
}

#[cfg(test)]
mod tests;
