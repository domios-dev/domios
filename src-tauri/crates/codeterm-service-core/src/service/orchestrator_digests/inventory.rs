use std::collections::HashSet;

use crate::multiplexer::types::{AgentRole, ShellActivity};
use crate::service::plan::digest_rules::PlanDigestCounters;
use crate::service::ServiceContext;
use codeterm_types::pty::TabStatus;

pub(crate) struct NoticeInventorySnapshot {
    enabled: bool,
    jobs: usize,
    tabs: Vec<InventoryTab>,
}

struct InventoryTab {
    id: String,
    title: String,
    owner: bool,
    agent: bool,
    live: bool,
    idle: bool,
    worker: bool,
    idle_utility: bool,
    task_id: Option<String>,
}

pub(crate) fn snapshot_notice_inventory(
    svc: &ServiceContext,
    plan_id: &str,
    team_id: Option<&str>,
    owner: Option<&str>,
) -> NoticeInventorySnapshot {
    let Some(team_id) = team_id else {
        return NoticeInventorySnapshot {
            enabled: false,
            jobs: 0,
            tabs: Vec::new(),
        };
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
    let mux = svc.mux.read();
    let tabs = mux
        .all_tabs()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| {
            let status =
                crate::multiplexer::state::effective_tab_status(tab, svc.resolved_tab_status(tab));
            InventoryTab {
                id: tab.id.clone(),
                title: tab.title.clone(),
                owner: owner == Some(tab.id.as_str()),
                agent: tab.agent_type.is_some() || tab.is_chat_backend(),
                live: (tab.pty_id.is_some() || tab.is_chat_backend()) && status != TabStatus::Dead,
                idle: status == TabStatus::Idle,
                worker: tab.role == Some(AgentRole::Worker),
                idle_utility: tab.is_utility_shell()
                    && !tab.utility_command_pending
                    && tab.shell_command_state.activity == ShellActivity::Idle,
                task_id: tab
                    .task_binding
                    .as_ref()
                    .filter(|binding| binding.plan_id.as_deref() == Some(plan_id))
                    .and_then(|binding| binding.task_id.clone()),
            }
        })
        .collect();
    NoticeInventorySnapshot {
        enabled: true,
        jobs: crons + scenarios,
        tabs,
    }
}

impl NoticeInventorySnapshot {
    pub(crate) fn targets(&self) -> codeterm_plan::service_plan::NoticeInventoryTargets {
        let mut tab_ids = self
            .tabs
            .iter()
            .filter(|tab| tab.worker)
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        let mut task_ids = self
            .tabs
            .iter()
            .filter(|tab| tab.worker)
            .filter_map(|tab| tab.task_id.clone())
            .collect::<Vec<_>>();
        tab_ids.sort();
        tab_ids.dedup();
        task_ids.sort();
        task_ids.dedup();
        codeterm_plan::service_plan::NoticeInventoryTargets { task_ids, tab_ids }
    }

    pub(super) fn populate(
        &self,
        finished: &[codeterm_plan::service_plan::NoticeFinishedBinding],
        counters: &mut PlanDigestCounters,
    ) {
        if !self.enabled {
            return;
        }
        counters.jobs = self.jobs;
        let tasks = finished
            .iter()
            .map(|binding| binding.task_id.as_str())
            .collect::<HashSet<_>>();
        let tabs = finished
            .iter()
            .filter_map(|binding| binding.assigned_tab_id.as_deref())
            .collect::<HashSet<_>>();
        for tab in &self.tabs {
            let finished_task = tab
                .task_id
                .as_deref()
                .is_some_and(|task| tasks.contains(task))
                || tabs.contains(tab.id.as_str());
            add_tab(
                counters,
                TeamTab {
                    id: &tab.id,
                    title: &tab.title,
                    owner: tab.owner,
                    agent: tab.agent,
                    live: tab.live,
                    idle: tab.idle,
                    finished_worker: tab.worker && finished_task,
                    idle_utility: tab.idle_utility,
                },
            );
        }
        counters.idle_workers.sort();
        counters.idle_workers.truncate(5);
    }
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
