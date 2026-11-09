use crate::plan::Plan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod builder;
mod metadata_input;
pub use builder::{is_activity_section, DigestBuilder};
pub use metadata_input::{append_metadata_task_rows, MetadataTaskInput};

pub const MAX_DIGEST_BYTES: usize = 8192;
pub const SECTIONS: [(&str, usize); 18] = [
    ("decisions", 10),
    ("owner questions", 10),
    ("waits", 10),
    ("review backlog", 1),
    ("time in state", 10),
    ("done but not shown", 10),
    ("agent capacity", 15),
    ("actionable", 20),
    ("tasks moved", 20),
    ("reports", 10),
    ("review verdicts", 10),
    ("incidents", 10),
    ("queues", 1),
    ("blueprints", 10),
    ("memory", 10),
    ("owner messages", 1),
    ("carried", 5),
    ("candidates", 3),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct Digest {
    pub body: String,
    pub activity: bool,
    pub section_sizes: BTreeMap<String, usize>,
}

pub struct PreparedSection {
    pub rows: Vec<Row>,
    pub total: usize,
}

pub type PreparedRows = BTreeMap<String, PreparedSection>;

pub(in crate::service_plan) fn clean(text: &str, cap: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .take(cap)
        .collect()
}

pub fn build(header: &str, rows: &BTreeMap<String, Vec<Row>>) -> Digest {
    build_bounded(header, rows, MAX_DIGEST_BYTES)
}

pub fn bounded_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn build_bounded(header: &str, rows: &BTreeMap<String, Vec<Row>>, limit: usize) -> Digest {
    let prepared = rows
        .iter()
        .map(|(name, rows)| {
            let mut rows = rows.clone();
            rows.sort();
            rows.dedup();
            if name == "time in state" {
                rows.sort_by(|a, b| b.summary.cmp(&a.summary).then_with(|| a.id.cmp(&b.id)));
            }
            (
                name.clone(),
                PreparedSection {
                    total: rows.len(),
                    rows,
                },
            )
        })
        .collect();
    build_prepared(header, &prepared, limit)
}

pub fn build_prepared(header: &str, rows: &PreparedRows, limit: usize) -> Digest {
    let activity = SECTIONS.iter().any(|(name, _)| {
        is_activity_section(name) && rows.get(*name).is_some_and(|section| section.total > 0)
    });
    let mut builder = DigestBuilder::new(header, limit, activity);
    for (name, cap) in SECTIONS {
        let Some(section) = rows.get(name).filter(|section| section.total > 0) else {
            continue;
        };
        if !builder.start_section(name, section.total) {
            break;
        }
        for row in section.rows.iter().take(cap) {
            if !builder.append(row) {
                break;
            }
        }
    }
    builder.finish()
}

pub fn metadata_rows(plan: &Plan, since: u64) -> BTreeMap<String, Vec<Row>> {
    let mut sections = BTreeMap::new();
    let actionable = super::super::state::orchestrator_actionable_task_ids(plan)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for task in &plan.tasks {
        append_metadata_task_rows(
            &mut sections,
            MetadataTaskInput {
                id: &task.id,
                name: &task.name,
                status: &task.status,
                changed_at: task.status_changed_at,
                prior: plan
                    .control_loop
                    .task_states
                    .get(&task.id)
                    .map(String::as_str),
                actionable: actionable.contains(&task.id),
            },
            since,
        );
    }
    sections
}

pub fn plan_rows(plan: &Plan, since: u64) -> BTreeMap<String, Vec<Row>> {
    let mut sections = metadata_rows(plan, since);
    for task in &plan.tasks {
        for verdict in task.review_evidence.iter().filter(|r| r.at > since) {
            sections
                .entry("review verdicts".into())
                .or_default()
                .push(Row {
                    id: task.id.clone(),
                    kind: "review".into(),
                    state: format!("{:?}", verdict.verdict).to_lowercase(),
                    summary: verdict.note.clone(),
                });
        }
        for attempt in &task.execution_attempts {
            if let Some(report) = attempt
                .report
                .as_ref()
                .filter(|_| attempt.reported_at.is_some_and(|at| at > since))
            {
                sections.entry("reports".into()).or_default().push(Row {
                    id: task.id.clone(),
                    kind: "report".into(),
                    state: report.status.clone().unwrap_or_else(|| "unknown".into()),
                    summary: format!(
                        "sha={} tests_covering={}",
                        report.sha.as_deref().unwrap_or("none"),
                        if report.message.as_deref().is_some_and(|m| m
                            .lines()
                            .any(|l| l.starts_with("TESTS COVERING CHANGES:")))
                        {
                            "yes"
                        } else {
                            "no"
                        }
                    ),
                });
            }
            if attempt.assigned_at.is_some_and(|at| at > since)
                && (attempt.interruption.reason.is_some() || attempt.acknowledged_at.is_none())
            {
                sections.entry("incidents".into()).or_default().push(Row {
                    id: attempt.id.clone(),
                    kind: "attempt".into(),
                    state: if attempt.acknowledged_at.is_none() {
                        "awaiting_ack"
                    } else {
                        "interrupted"
                    }
                    .into(),
                    summary: task.id.clone(),
                });
            }
        }
    }
    for stride in plan
        .strides
        .iter()
        .chain(plan.tasks.iter().flat_map(|t| t.milestones.iter()))
        .filter(|m| m.ts > since && m.kind != "curator_round")
    {
        let section = match stride.kind.as_str() {
            "report" | "worker_report" => "reports",
            "incident" | "worker_lost" | "missing_ack" => "incidents",
            "skill_ledger" | "skill" => "memory",
            _ => continue,
        };
        sections.entry(section.into()).or_default().push(Row {
            id: stride.id.clone(),
            kind: stride.kind.clone(),
            state: "recorded".into(),
            summary: stride.text.clone(),
        });
    }
    for learning in plan
        .heartbeat
        .learnings
        .iter()
        .filter(|l| l.tag.as_deref() == Some("curator:next"))
    {
        let references_open = plan
            .tasks
            .iter()
            .any(|t| !t.status.is_terminal() && learning.text.contains(&t.id));
        if references_open {
            sections.entry("carried".into()).or_default().push(Row {
                id: learning.id.clone(),
                kind: "next-check".into(),
                state: "open".into(),
                summary: learning.text.clone(),
            });
        }
    }
    let mut episodes = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for task in plan
        .tasks
        .iter()
        .filter(|t| t.status == crate::plan::TaskStatus::Done)
    {
        for skill in task
            .execution_attempts
            .iter()
            .filter_map(|a| a.report.as_ref())
            .flat_map(|r| r.loaded_plan_skills.iter())
        {
            episodes
                .entry(skill.slug.clone())
                .or_default()
                .insert(task.id.clone());
        }
    }
    for (slug, ids) in episodes.into_iter().filter(|(_, ids)| ids.len() >= 2) {
        sections.entry("candidates".into()).or_default().push(Row {
            id: format!("plan-skill:{slug}"),
            kind: "plan_skill".into(),
            state: "repeated".into(),
            summary: ids.into_iter().collect::<Vec<_>>().join(" "),
        });
    }
    sections
}
