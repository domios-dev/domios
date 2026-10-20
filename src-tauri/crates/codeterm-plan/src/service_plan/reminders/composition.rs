use crate::plan::{Plan, Task};

pub const MAX_REMINDER_BYTES: usize = 4096;
pub const ROLE_HEADER: &str = "You are the Orchestrator of Team {team} for plan {plan} ({title}).\nManual: codeterm docs ORCHESTRATION (operations: ORCHESTRATION-OPERATIONS, tasks: ORCHESTRATION-TASKS).";
pub const MORE: &str = "\nMore: codeterm orchestrator status --plan ";
pub const TASK_NAME_CHARS: usize = 64;
pub const REVIEW: &str = "Review";
pub const DISPATCH: &str = "Dispatch";
pub const INCIDENTS: &str = "Incidents";
pub const MAX_GROUP_BYTES: usize = 512;

pub fn action_groups(plan: &Plan) -> String {
    use crate::service_plan::state::{task_projection, TaskPresentation};
    let actionable = crate::service_plan::state::orchestrator_actionable_task_ids(plan);
    let mut groups = [Vec::new(), Vec::new(), Vec::new()];
    for task in &plan.tasks {
        if !actionable.contains(&task.id) {
            continue;
        }
        let group = match task_projection(plan, task).presentation {
            TaskPresentation::InReview | TaskPresentation::QueuedForQa => 0,
            TaskPresentation::ActionablePending => 1,
            TaskPresentation::HandoffRequired | TaskPresentation::Blocked => 2,
            _ => continue,
        };
        groups[group].push(task_label(task));
    }
    [REVIEW, DISPATCH, INCIDENTS]
        .into_iter()
        .zip(groups)
        .filter_map(|(title, labels)| {
            if labels.is_empty() {
                return None;
            }
            let mut selected = Vec::new();
            let mut bytes = title.len() + 32;
            for label in &labels {
                if selected.len() == 5 || bytes + label.len() + 2 > MAX_GROUP_BYTES {
                    break;
                }
                bytes += label.len() + 2;
                selected.push(label.as_str());
            }
            let extra = labels.len().saturating_sub(selected.len());
            let listed = selected.join(", ");
            Some(if extra > 0 {
                format!("{title}: {listed} (+{extra} more)")
            } else {
                format!("{title}: {listed}")
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn short(text: &str, count: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(count)
        .collect()
}

pub fn task_label(task: &Task) -> String {
    let name = short(&task.name, TASK_NAME_CHARS);
    match task.key.as_deref().or(task.handle.as_deref()) {
        Some(key) => format!("{} {name}", short(key, 32)),
        None => name,
    }
}

pub fn role_header(plan: &Plan) -> String {
    ROLE_HEADER
        .replace(
            "{team}",
            &short(plan.team_id.as_deref().unwrap_or("unbound"), 64),
        )
        .replace("{plan}", &short(&plan.id, 64))
        .replace("{title}", &short(&plan.title, 128))
}

pub fn compose(plan: &Plan, fragments: &[&str]) -> String {
    let mut text = role_header(plan);
    let footer = format!("{MORE}{}", short(&plan.id, 64));
    'fragments: for fragment in fragments {
        let fragment = label_task_references(plan, fragment);
        for line in fragment.lines() {
            if text.len() + line.len() + 1 + footer.len() > MAX_REMINDER_BYTES {
                text.push_str(&footer);
                break 'fragments;
            }
            text.push('\n');
            text.push_str(line);
        }
    }
    text
}

pub fn label_task_references(plan: &Plan, text: &str) -> String {
    fn delimiter(ch: char) -> bool {
        !ch.is_alphanumeric() && ch != '_' && ch != '-'
    }
    text.split_inclusive(delimiter)
        .map(|part| {
            let token = part.trim_end_matches(delimiter);
            match plan.tasks.iter().find(|task| task.id == token) {
                Some(task) => format!("{}{}", task_label(task), &part[token.len()..]),
                None => part.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
