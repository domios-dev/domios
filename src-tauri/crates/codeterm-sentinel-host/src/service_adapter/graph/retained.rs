use super::*;
use codeterm_blueprint_types::blueprint_document::Document;

type Definitions = BTreeMap<String, BTreeMap<u64, Value>>;

pub(super) fn definitions(
    handle: &Handle,
    jobs: &[Sentinel],
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
) -> Result<Definitions, String> {
    let mut definitions = Definitions::new();
    for job in jobs.iter().filter(|job| job.is_scenario_root()) {
        let Some(current) = &job.document else {
            continue;
        };
        let mut versions = BTreeMap::from([(job.definition_version, current.clone())]);
        let needs_history = journals.get(&job.id).is_some_and(|entries| {
            entries.iter().any(|row| {
                row.definition_version
                    .is_some_and(|v| v != job.definition_version)
            })
        });
        if needs_history {
            let pending = { handle.store.lock().typed_history(&job.id)? };
            let history = match pending {
                Some(pending) => storage::history_versions(
                    pending
                        .wait(storage::COMMIT_DEADLINE)
                        .map_err(|error| error.to_string())?
                        .value,
                ),
                None => crate::amend::history(job).versions,
            };
            for version in history {
                if let Some(document) = version.definition.document {
                    versions.entry(version.version).or_insert(document);
                }
            }
        }
        definitions.insert(job.id.clone(), versions);
    }
    Ok(definitions)
}

pub(super) fn project(
    snapshot: &mut Snapshot,
    jobs: &[Sentinel],
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
    definitions: &Definitions,
) -> Result<(), String> {
    for job in jobs
        .iter()
        .filter(|job| job.is_scenario_root() && job.document.is_some())
    {
        if !snapshot.nodes.iter().any(|node| {
            node.pointer("/identity/kind").and_then(Value::as_str) == Some("blueprint")
                && node.pointer("/identity/id").and_then(Value::as_str) == Some(&job.id)
        }) {
            continue;
        }
        let mut entries = journals.get(&job.id).cloned().unwrap_or_default();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.entry_id));
        let mut seen = BTreeSet::new();
        let mut runs = Vec::new();
        for entry in &entries {
            if !seen.insert(entry.run_id.as_str()) {
                continue;
            }
            let rows = entries
                .iter()
                .filter(|row| row.run_id == entry.run_id)
                .cloned()
                .collect::<Vec<_>>();
            let projection = run_projection(job, &rows, definitions.get(&job.id))?;
            runs.push((entry.run_id.clone(), projection));
        }
        let latest = match runs.first() {
            Some((_, projection)) => projection.clone(),
            None => run_projection(job, &[], definitions.get(&job.id))?,
        };
        let index = match snapshot
            .blueprints
            .iter()
            .position(|view| view["id"] == job.id)
        {
            Some(index) => index,
            None => {
                snapshot.blueprints.push(watch_view(job));
                snapshot.blueprints.len() - 1
            }
        };
        let view = &mut snapshot.blueprints[index];
        extend(view, latest);
        if job.pipeline().is_none() {
            view["runs"] = json!(runs
                .iter()
                .map(|(id, _)| watch_run(id, &entries))
                .collect::<Vec<_>>());
            view["run_id"] = json!(runs.first().map(|(id, _)| id));
        }
        if let Some(items) = view["runs"].as_array_mut() {
            for item in items {
                if let Some((_, projection)) = runs.iter().find(|(id, _)| item["id"] == *id) {
                    extend(item, projection.clone());
                }
            }
        }
    }
    Ok(())
}

fn run_projection(
    job: &Sentinel,
    rows: &[BlueprintJournalEntry],
    versions: Option<&BTreeMap<u64, Value>>,
) -> Result<Value, String> {
    let version = if rows.is_empty() {
        Some(job.definition_version)
    } else {
        rows[0].definition_version
    };
    let document = version
        .and_then(|version| versions?.get(&version))
        .and_then(|value| serde_json::from_value::<Document>(value.clone()).ok());
    crate::blueprint_calls::project_retained(document.as_ref(), rows, version)
}

fn extend(target: &mut Value, projection: Value) {
    if let (Some(target), Value::Object(fields)) = (target.as_object_mut(), projection) {
        target.extend(fields);
    }
}

fn watch_view(job: &Sentinel) -> Value {
    let owner = job
        .document
        .as_ref()
        .and_then(|doc| doc["owner"].as_str())
        .unwrap_or("tab");
    json!({"id":job.id,"name":job.name,"note":job.note,"stopped_at":job.cleanup.stopped_at,"deletes_at":job.cleanup.deletes_at,
        "owner":{"kind":owner,"tab_id":if owner == "team" {None} else {Some(&job.creator_tab_id)},"team_id":job.team_id},
        "state":if job.machine.stopped {"stopped"} else if !job.admission.permits_steps() || job.machine.paused || job.machine.held.is_some() {"held"} else {"idle"},
        "held_reason":crate::plan_lock::held_reason(job),
        "run_id":null,"current_step":null,"last_event_at_ms":null,"steps":[],"runs":[]})
}

fn watch_run(id: &str, entries: &[BlueprintJournalEntry]) -> Value {
    use codeterm_blueprint_types::blueprint_journal::JournalState;
    let rows = entries
        .iter()
        .filter(|row| row.run_id == id)
        .collect::<Vec<_>>();
    let (closed, current) = BlueprintJournalEntry::derive_run(&rows);
    let state = match closed {
        Some(JournalState::Failed) => "failed",
        Some(JournalState::Held) => "held",
        Some(_) => "completed",
        None => "running",
    };
    json!({"id":id,"state":state,"closed":closed.is_some(),"current_step":current,"steps":[],
        "started_at_ms":rows.iter().map(|row| row.started_at_ms).min(),
        "last_event_at_ms":rows.iter().map(|row| row.ended_at_ms.unwrap_or(row.started_at_ms)).max()})
}

#[cfg(test)]
mod tests;
