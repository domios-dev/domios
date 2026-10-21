use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn memory(
    svc: &ServiceContext,
    plan: &Plan,
    since: u64,
    rows: &mut BTreeMap<String, Vec<digest::Row>>,
) {
    let Some(since) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(since as i64) else {
        return;
    };
    let since = since.to_rfc3339();
    let Ok(entries) = svc.mem.tail_recent(Some(&since), 64) else {
        return;
    };
    let mut kept = 0;
    for entry in entries {
        if kept >= 5 {
            break;
        }
        let scoped = svc
            .mem
            .get(&entry.id)
            .ok()
            .flatten()
            .is_some_and(|item| item.tags.iter().any(|tag| tag == &plan.id));
        if scoped {
            rows.entry("memory".into()).or_default().push(digest::Row {
                id: entry.id,
                kind: entry.kind,
                state: entry.changed_by,
                summary: entry.title,
            });
            kept += 1;
        }
    }
}

pub(super) fn blueprints(
    svc: &ServiceContext,
    plan: &Plan,
    since: u64,
    now: u64,
    rows: &mut BTreeMap<String, Vec<digest::Row>>,
) -> Vec<String> {
    let Some(ports) = svc.try_ext::<crate::ports::sentinel::SentinelPorts>() else {
        return vec![];
    };
    let mut records = ports
        .store
        .records()
        .into_iter()
        .filter(|r| r.team_id == plan.team_id)
        .collect::<Vec<_>>();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let mut sources = BTreeMap::<String, String>::new();
    let mut flagged = BTreeSet::new();
    for record in records {
        let d = &record.definition;
        let transitions = d["transitions"].as_array();
        let last = transitions
            .into_iter()
            .flatten()
            .filter_map(|t| t["accepted_at"].as_u64())
            .max();
        let first = transitions
            .into_iter()
            .flatten()
            .filter_map(|t| t["accepted_at"].as_u64())
            .min();
        let source = if let Some(probe) = d["probe"].as_str().filter(|p| !p.is_empty()) {
            Some(format!("probe:{probe}"))
        } else {
            d.get("observe")
                .filter(|v| !v.is_null())
                .map(|v| format!("observe:{v}"))
        };
        let mut reasons = vec![];
        if !d["machine"]["stopped"].as_bool().unwrap_or(false) {
            if let Some(source) = source {
                if let Some(prior) = sources.get(&source) {
                    reasons.push(format!("duplicate of {prior}"));
                    if record.scenario_root {
                        flagged.insert(record.id.clone());
                    }
                } else {
                    sources.insert(source, record.id.clone());
                }
            }
            if let (Some(first), Some(last), Some(expiry)) = (first, last, d["expires_at"].as_u64())
            {
                if now.saturating_sub(last) > 86_400_000
                    && now.saturating_sub(first) > expiry.saturating_sub(first) / 2
                {
                    reasons.push("stale".into());
                    if record.scenario_root {
                        flagged.insert(record.id.clone());
                    }
                }
            }
        }
        if d["machine"]["held"].is_string() {
            reasons.push("held escalation".into());
        }
        let mut causes = BTreeMap::<&str, usize>::new();
        for amendment in d["amendments"].as_array().into_iter().flatten() {
            if let Some(cause) = amendment["cause_transition_id"].as_str() {
                *causes.entry(cause).or_default() += 1;
            }
        }
        for (cause, count) in causes.into_iter().filter(|(_, count)| *count >= 2).take(3) {
            reasons.push(format!("cause {cause} repeated {count}"));
        }
        let recent = last.is_some_and(|at| at > since)
            || d["stopped_at"].as_u64().is_some_and(|at| at > since)
            || d["amendments"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|amendment| amendment["at"].as_u64().is_some_and(|at| at > since));
        if recent || !reasons.is_empty() {
            rows.entry("blueprints".into())
                .or_default()
                .push(digest::Row {
                    id: record.id.clone(),
                    kind: if record.scenario_root {
                        "blueprint"
                    } else {
                        "stage"
                    }
                    .into(),
                    state: if d["machine"]["stopped"] == true {
                        "stopped"
                    } else if d["machine"]["held"].is_string() {
                        "held"
                    } else {
                        "running"
                    }
                    .into(),
                    summary: reasons.join("; "),
                });
        }
    }
    flagged.into_iter().collect()
}
