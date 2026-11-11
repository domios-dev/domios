use super::*;
use codeterm_blueprint_types::blueprint_journal::{JournalMarker, JournalOutcome, JournalState};
use codeterm_sentinel::blueprint_snapshot::{
    self as views, BlueprintResultState, BlueprintRunState, BlueprintState, BlueprintStepState,
};

pub(super) fn project_pipeline_contract(
    snapshot: &mut Snapshot,
    jobs: &[Sentinel],
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
    live_leases: &BTreeSet<String>,
    event_subscriptions: &codeterm_plugin::graph_events::SubscriptionTable,
) {
    let mut journal_revisions = jobs
        .iter()
        .filter(|job| job.document.is_some())
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
                    codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
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
    for job in jobs.iter().filter(|job| graph::is_snapshot_root(job)) {
        let Some(node) = snapshot
            .nodes
            .iter_mut()
            .find(|node| node.identity.id == job.id && node.identity.kind == "blueprint")
        else {
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
        } else if !job.admission.permits_steps()
            || job.machine.paused
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
        } else if latest.closed {
            if latest.state == BlueprintRunState::Failed {
                "failed"
            } else if latest.state == BlueprintRunState::Held {
                "held"
            } else {
                "idle"
            }
        } else if live_leases.contains(&job.id) {
            "running"
        } else {
            match latest.state {
                BlueprintRunState::Running => "running",
                BlueprintRunState::Failed => "failed",
                BlueprintRunState::Held => "held",
                _ => "idle",
            }
        };
        let (owner_kind, owner_tab) = match spec.owner {
            codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Tab => {
                ("tab", Some(job.creator_tab_id.as_str()))
            }
            codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Team => ("team", None),
        };
        let mut blueprint = views::Snapshot {
            id: job.id.clone(),
            name: Some(spec.name.clone()),
            note: job.note.clone(),
            stopped_at: job.cleanup.stopped_at,
            deletes_at: job.cleanup.deletes_at,
            owner: views::Owner {
                kind: owner_kind.into(),
                tab_id: owner_tab.map(str::to_owned),
                team_id: job.team_id.clone(),
            },
            state: match state {
                "stopped" => BlueprintState::Stopped,
                "held" => BlueprintState::Held,
                "failed" => BlueprintState::Failed,
                "running" => BlueprintState::Running,
                _ => BlueprintState::Idle,
            },
            run_id: latest.id.clone(),
            current_step: latest.current_step,
            last_event_at_ms: latest.last_event_at_ms,
            trigger: Some(trigger_view(&spec.trigger)),
            steps: latest.steps.clone(),
            result: latest.result.clone(),
            runs,
            plugin_hold: None,
            held_reason: None,
            event_subscription: None,
            projection: Default::default(),
        };
        if let Some(hold) = &job.admission.plugin_hold {
            blueprint.plugin_hold = Some(hold.clone());
        }
        if state == "held" {
            let latest_run = latest.id.as_deref();
            let journal_reason = entries
                .iter()
                .filter(|entry| {
                    Some(entry.run_id.as_str()) == latest_run && entry.state == JournalState::Held
                })
                .find_map(|entry| entry.reason.as_ref())
                .map(|reason| reason.as_str().to_string());
            let held_reason = crate::plan_lock::held_reason(job)
                .map(str::to_string)
                .or_else(|| {
                    job.admission
                        .plugin_hold
                        .as_ref()
                        .map(|hold| hold.reason.clone())
                })
                .or_else(|| {
                    job.machine
                        .held
                        .clone()
                        .or_else(|| job.machine.paused.then(|| "paused".to_string()))
                        .or_else(|| job.last_reason.clone())
                        .or(journal_reason)
                });
            if let Some(reason) = held_reason {
                blueprint.held_reason = Some(Some(reason));
            }
        }
        if matches!(
            &spec.trigger,
            codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
        ) {
            let subscription = event_subscriptions.summary(&job.id);
            blueprint.event_subscription = Some(views::EventSubscription {
                state: subscription
                    .as_ref()
                    .map(|item| item.state.as_str())
                    .unwrap_or(if job.machine.stopped {
                        "stopped"
                    } else {
                        "unavailable"
                    })
                    .into(),
                delivered: subscription
                    .as_ref()
                    .map(|item| item.delivered)
                    .unwrap_or_default(),
                refused: subscription
                    .as_ref()
                    .map(|item| item.refused)
                    .unwrap_or_default(),
                last_event_at_ms: subscription.as_ref().and_then(|item| item.last_event_at_ms),
                reason: subscription
                    .as_ref()
                    .and_then(|item| item.reason.clone())
                    .or_else(|| (!job.machine.stopped).then(|| "not_subscribed".into())),
            });
        }
        node.state = Some(state.into());
        node.held_reason = Some(blueprint.held_reason.clone().flatten());
        snapshot.blueprints.push(blueprint);
    }
}

fn pipeline_run_view(
    job: &Sentinel,
    spec: &codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec,
    entries: &[&BlueprintJournalEntry],
) -> views::Run {
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
                .find(|entry| entry.marker() == JournalMarker::Step(index as u32));
            let summary = entries.iter().find(|entry| {
                entry.marker() == JournalMarker::Step(index as u32)
                    && entry.result_summary.is_some()
            });
            pipeline_step_view(index, &step.name, &step.kind, entry, summary)
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
    views::Run {
        id: entries.first().map(|entry| entry.run_id.clone()),
        state: match state {
            "failed" => BlueprintRunState::Failed,
            "held" => BlueprintRunState::Held,
            "completed" => BlueprintRunState::Completed,
            "running" => BlueprintRunState::Running,
            _ => BlueprintRunState::Idle,
        },
        closed: closed_state.is_some(),
        started_at_ms: entries.iter().map(|entry| entry.started_at_ms).min(),
        last_event_at_ms: entries
            .iter()
            .map(|entry| entry.ended_at_ms.unwrap_or(entry.started_at_ms))
            .max(),
        current_step: current,
        steps,
        result: Some(views::ResultView {
            target: result_target_view(&spec.result.to, spec.owner, job),
            state: match result_state {
                "delivered" => BlueprintResultState::Delivered,
                "failed" => BlueprintResultState::Failed,
                "delivering" => BlueprintResultState::Delivering,
                _ => BlueprintResultState::Waiting,
            },
            reason: result
                .and_then(|entry| entry.reason.as_ref().map(|reason| reason.as_str().into())),
            body_ref: result.and_then(|entry| entry.body_ref.clone()),
        }),
        projection: Default::default(),
    }
}

fn pipeline_step_view(
    index: usize,
    name: &str,
    kind: &codeterm_blueprint_types::blueprint_pipeline::StepKind,
    entry: Option<&&BlueprintJournalEntry>,
    summary: Option<&&BlueprintJournalEntry>,
) -> views::Step {
    let source = match kind {
        codeterm_blueprint_types::blueprint_pipeline::StepKind::Github { probe } => {
            Some(views::StepSource::Github(probe.clone()))
        }
        codeterm_blueprint_types::blueprint_pipeline::StepKind::Http { probe } => {
            Some(views::StepSource::Http(views::HttpSource {
                url: probe.display_url(),
                expected_status: probe.expected_status.clone(),
                timeout_ms: probe.timeout_ms,
            }))
        }
        _ => None,
    };
    let source_state = if source.is_some() {
        entry
            .and_then(|entry| entry.outcome.as_ref())
            .map(|outcome| outcome.as_str())
            .filter(|state| {
                matches!(
                    *state,
                    "done" | "failed" | "failing" | "online" | "offline" | "up" | "down"
                )
            })
    } else {
        None
    };
    let kind = kind.name();
    views::Step {
        index,
        name: name.into(),
        kind: kind.into(),
        title: format!("{}: {name}", kind),
        state: entry.map_or(
            BlueprintStepState::Waiting(views::WaitingStep::Waiting),
            |entry| BlueprintStepState::Recorded(entry.state),
        ),
        outcome: entry
            .filter(|entry| !entry.outcome.as_ref().is_some_and(JournalOutcome::is_skip))
            .and_then(|entry| {
                entry
                    .outcome
                    .as_ref()
                    .map(|outcome| outcome.as_str().into())
            }),
        started_at_ms: entry.map(|entry| entry.started_at_ms),
        ended_at_ms: entry.and_then(|entry| entry.ended_at_ms),
        result_summary: summary.and_then(|entry| entry.result_summary.clone()),
        reason: entry.and_then(|entry| entry.reason.as_ref().map(|reason| reason.as_str().into())),
        source_state: source.as_ref().map(|_| source_state.map(str::to_owned)),
        source,
    }
}

fn trigger_view(trigger: &codeterm_blueprint_types::blueprint_pipeline::Trigger) -> views::Trigger {
    use codeterm_blueprint_types::blueprint_pipeline::Trigger;
    match trigger {
        Trigger::Webhook { .. } => views::Trigger {
            r#type: "webhook".into(),
            summary: "authenticated JSON webhook".into(),
        },
        Trigger::Schedule { every } => views::Trigger {
            r#type: "schedule".into(),
            summary: format!("every {every}"),
        },
        Trigger::Event { kind, .. } => views::Trigger {
            r#type: "event".into(),
            summary: format!("event {kind}"),
        },
        Trigger::Input { kind, source } => views::Trigger {
            r#type: "input".into(),
            summary: input_summary(*kind, source),
        },
    }
}

fn input_summary(
    kind: codeterm_blueprint_types::sentinel_input::MediaKind,
    source: &codeterm_blueprint_types::sentinel_input::DataRef,
) -> String {
    use codeterm_blueprint_types::sentinel_input::DataRef;
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
    target: &codeterm_blueprint_types::blueprint_pipeline::ResultTarget,
    owner: codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner,
    job: &Sentinel,
) -> views::ResultTarget {
    use codeterm_blueprint_types::blueprint_pipeline::{BlueprintOwner, ResultTarget};
    match target {
        ResultTarget::Owner if owner == BlueprintOwner::Team => views::ResultTarget {
            kind: "team".into(),
            id: job.team_id.clone(),
        },
        ResultTarget::Owner => views::ResultTarget {
            kind: "owner_tab".into(),
            id: Some(job.creator_tab_id.clone()),
        },
        ResultTarget::Tab(id) => views::ResultTarget {
            kind: "tab".into(),
            id: Some(id.clone()),
        },
        ResultTarget::Team => views::ResultTarget {
            kind: "team".into(),
            id: job.team_id.clone(),
        },
        ResultTarget::Notify => views::ResultTarget {
            kind: "notify".into(),
            id: None,
        },
    }
}

#[cfg(test)]
mod tests;
