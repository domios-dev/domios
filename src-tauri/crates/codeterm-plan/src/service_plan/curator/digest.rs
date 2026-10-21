use crate::plan::Plan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

fn clean(text: &str, cap: usize) -> String {
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
    let limit = limit.clamp(1, MAX_DIGEST_BYTES);
    let mut body = format!("{}\n", bounded_text(header, limit.saturating_sub(1)));
    let mut sizes = BTreeMap::new();
    let activity = SECTIONS.iter().any(|(name, _)| {
        !matches!(
            *name,
            "actionable"
                | "owner questions"
                | "waits"
                | "review backlog"
                | "time in state"
                | "agent capacity"
                | "queues"
        ) && rows.get(*name).is_some_and(|rows| !rows.is_empty())
    });
    for (name, cap) in SECTIONS {
        let Some(rows) = rows.get(name).filter(|r| !r.is_empty()) else {
            continue;
        };
        let mut rows = rows.clone();
        rows.sort();
        rows.dedup();
        if name == "time in state" {
            rows.sort_by(|a, b| b.summary.cmp(&a.summary).then_with(|| a.id.cmp(&b.id)));
        }
        let start = body.len();
        let title = format!("\n{name}:\n");
        if body.len() + title.len() + 16 > limit {
            break;
        }
        body.push_str(&title);
        let mut emitted = 0;
        for row in rows.iter().take(cap) {
            let line = format!(
                "{} | {} | {} | {}\n",
                clean(&row.id, 64),
                clean(&row.kind, 32),
                clean(&row.state, 32),
                clean(&row.summary, 120)
            );
            if body.len() + line.len() + 24 > limit {
                break;
            }
            body.push_str(&line);
            emitted += 1;
        }
        if rows.len() > emitted {
            body.push_str(&format!("+{} more\n", rows.len() - emitted));
        }
        sizes.insert(name.into(), body.len() - start);
    }
    Digest {
        body,
        activity,
        section_sizes: sizes,
    }
}

pub fn plan_rows(plan: &Plan, since: u64) -> BTreeMap<String, Vec<Row>> {
    let mut sections = BTreeMap::<String, Vec<Row>>::new();
    for id in super::super::state::orchestrator_actionable_task_ids(plan) {
        sections.entry("actionable".into()).or_default().push(Row {
            id,
            kind: "task".into(),
            state: "waiting".into(),
            summary: String::new(),
        });
    }
    for task in &plan.tasks {
        if task.status_changed_at > since {
            sections.entry("tasks moved".into()).or_default().push(Row {
                id: task.id.clone(),
                kind: "task".into(),
                state: format!(
                    "{} -> {}",
                    plan.control_loop
                        .task_states
                        .get(&task.id)
                        .map(String::as_str)
                        .unwrap_or("unknown"),
                    task.status.key()
                ),
                summary: task.name.clone(),
            });
        }
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
