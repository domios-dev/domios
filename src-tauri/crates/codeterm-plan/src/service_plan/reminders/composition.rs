use crate::plan::{Plan, Task};

pub const MAX_REMINDER_BYTES: usize = 4096;
pub const ROLE_HEADER: &str = "You are the Orchestrator of Team {team} for plan {plan} ({title}).\nManual: domios docs ORCHESTRATION (operations: ORCHESTRATION-OPERATIONS, tasks: ORCHESTRATION-TASKS).";
pub const MORE: &str = "\nMore: domios orchestrator status --plan ";
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
    let groups = groups.map(|labels| ActionGroup {
        total: labels.len(),
        labels,
    });
    render_action_groups(groups)
}

pub(in crate::service_plan) struct ActionGroup {
    pub labels: Vec<String>,
    pub total: usize,
}

pub(in crate::service_plan) fn render_action_groups(groups: [ActionGroup; 3]) -> String {
    [REVIEW, DISPATCH, INCIDENTS]
        .into_iter()
        .zip(groups)
        .filter_map(|(title, group)| {
            if group.total == 0 {
                return None;
            }
            let mut selected = Vec::new();
            let mut bytes = title.len() + 32;
            for label in &group.labels {
                if selected.len() == 5 || bytes + label.len() + 2 > MAX_GROUP_BYTES {
                    break;
                }
                bytes += label.len() + 2;
                selected.push(label.as_str());
            }
            let extra = group.total.saturating_sub(selected.len());
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
        .enumerate()
        .flat_map(|(index, word)| (index > 0).then_some(' ').into_iter().chain(word.chars()))
        .take(count)
        .collect()
}

pub fn task_label(task: &Task) -> String {
    task_label_fields(&task.name, task.key.as_deref(), task.handle.as_deref())
}

pub(in crate::service_plan) fn task_label_fields(
    name: &str,
    key: Option<&str>,
    handle: Option<&str>,
) -> String {
    let name = short(name, TASK_NAME_CHARS);
    match key.or(handle) {
        Some(key) => format!("{} {name}", short(key, 32)),
        None => name,
    }
}

pub fn role_header(plan: &Plan) -> String {
    role_header_fields(&plan.id, &plan.title, plan.team_id.as_deref())
}

pub(in crate::service_plan) fn role_header_fields(
    id: &str,
    title: &str,
    team: Option<&str>,
) -> String {
    ROLE_HEADER
        .replace("{team}", &short(team.unwrap_or("unbound"), 64))
        .replace("{plan}", &short(id, 64))
        .replace("{title}", &short(title, 128))
}

pub(in crate::service_plan) struct ReminderComposer {
    text: String,
    footer: String,
    full: bool,
}

impl ReminderComposer {
    pub fn new(id: &str, title: &str, team: Option<&str>) -> Self {
        Self::with_header(id, role_header_fields(id, title, team))
    }
    pub fn with_header(id: &str, header: String) -> Self {
        Self {
            text: header,
            footer: format!("{MORE}{}", short(id, 64)),
            full: false,
        }
    }
    pub fn append(&mut self, labeled_fragment: &str) -> bool {
        if self.full {
            return false;
        }
        for line in labeled_fragment.lines() {
            if self.text.len() + line.len() + 1 + self.footer.len() > MAX_REMINDER_BYTES {
                self.text.push_str(&self.footer);
                self.full = true;
                return false;
            }
            self.text.push('\n');
            self.text.push_str(line);
        }
        true
    }
    pub fn finish(self) -> String {
        self.text
    }
}

pub fn compose(plan: &Plan, fragments: &[&str]) -> String {
    let mut composer = ReminderComposer::new(&plan.id, &plan.title, plan.team_id.as_deref());
    for fragment in fragments {
        if !composer.append(&label_task_references(plan, fragment)) {
            break;
        }
    }
    composer.finish()
}

pub fn label_task_references(plan: &Plan, text: &str) -> String {
    label_references(text, |token| {
        plan.tasks
            .iter()
            .find(|task| task.id == token)
            .map(task_label)
    })
}

pub(in crate::service_plan) fn reference_delimiter(ch: char) -> bool {
    !ch.is_alphanumeric() && ch != '_' && ch != '-'
}

pub(in crate::service_plan) fn label_references(
    text: &str,
    mut label: impl FnMut(&str) -> Option<String>,
) -> String {
    text.split_inclusive(reference_delimiter)
        .map(|part| {
            let token = part.trim_end_matches(reference_delimiter);
            match label(token) {
                Some(label) => format!("{}{}", label, &part[token.len()..]),
                None => part.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
