use std::collections::HashSet;

use crate::multiplexer::types::{AgentRole, ShellActivity};
use crate::plan::{Plan, TaskStatus};
use crate::service::plan::digest_rules::PlanDigestCounters;
use crate::service::ServiceContext;
use codeterm_types::pty::TabStatus;

pub(super) fn populate(svc: &ServiceContext, plan: &Plan, counters: &mut PlanDigestCounters) {
    let Some(team_id) = plan.team_id.as_deref() else {
        return;
    };
    let crons = svc
        .try_ext::<crate::CronState>()
        .map(|state| state.lock().list_for_team(team_id).len())
        .unwrap_or_default();
    let scenarios = svc
        .try_ext::<crate::ports::sentinel::SentinelPorts>()
        .map(|ports| {
            let records = ports.store.records();
            count_job_roots(
                team_id,
                records.iter().map(|job| JobRoot {
                    workspace: &job.workspace_id,
                    id: &job.id,
                    team: job.team_id.as_deref(),
                    scenario_root: job.scenario_root,
                }),
            )
        })
        .unwrap_or_default();
    counters.jobs = crons + scenarios;
    let finished_workers: HashSet<_> = plan
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .map(|task| task.id.as_str())
        .collect();
    let finished_tabs: HashSet<_> = plan
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .filter_map(|task| task.assigned_tab_id.as_deref())
        .collect();
    let mux = svc.mux.read();
    let tabs = mux
        .all_tabs()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id));
    for tab in tabs {
        let status =
            crate::multiplexer::state::effective_tab_status(tab, svc.resolved_tab_status(tab));
        let finished_task = tab.task_binding.as_ref().is_some_and(|binding| {
            binding.plan_id.as_deref() == Some(plan.id.as_str())
                && binding
                    .task_id
                    .as_deref()
                    .is_some_and(|id| finished_workers.contains(id))
        }) || finished_tabs.contains(tab.id.as_str());
        let row = TeamTab {
            id: &tab.id,
            title: &tab.title,
            owner: plan.owner.tab_id.as_deref() == Some(tab.id.as_str()),
            agent: tab.agent_type.is_some() || tab.is_chat_backend(),
            live: (tab.pty_id.is_some() || tab.is_chat_backend()) && status != TabStatus::Dead,
            idle: status == TabStatus::Idle,
            finished_worker: tab.role == Some(AgentRole::Worker) && finished_task,
            idle_utility: tab.is_utility_shell()
                && !tab.utility_command_pending
                && tab.shell_command_state.activity == ShellActivity::Idle,
        };
        add_tab(counters, row);
    }
    counters.idle_workers.sort();
    counters.idle_workers.truncate(5);
}

struct JobRoot<'a> {
    workspace: &'a str,
    id: &'a str,
    team: Option<&'a str>,
    scenario_root: bool,
}

fn count_job_roots<'a>(team: &str, jobs: impl Iterator<Item = JobRoot<'a>>) -> usize {
    jobs.filter(|job| job.team == Some(team) && job.scenario_root)
        .map(|job| (job.workspace, job.id))
        .collect::<HashSet<_>>()
        .len()
}

struct TeamTab<'a> {
    id: &'a str,
    title: &'a str,
    owner: bool,
    agent: bool,
    live: bool,
    idle: bool,
    finished_worker: bool,
    idle_utility: bool,
}

fn add_tab(counters: &mut PlanDigestCounters, tab: TeamTab<'_>) {
    if tab.owner {
        return;
    }
    if tab.agent && tab.live {
        counters.agent_tabs += 1;
        if tab.idle && tab.finished_worker {
            counters
                .idle_workers
                .push(format!("{} ({})", tab.title, tab.id));
        }
    } else if !tab.agent && tab.idle_utility {
        counters.idle_utility_shells += 1;
    }
}

#[cfg(test)]
mod tests;
