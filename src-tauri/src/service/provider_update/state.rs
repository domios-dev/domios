use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Coordinator {
    pub runs: BTreeMap<String, UpdateRun>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct UpdateRun {
    pub provider: String,
    pub current: String,
    pub target: String,
    pub outcome: RunOutcome,
    pub owner_pid: u32,
    pub restart_owners: BTreeMap<String, u32>,
    pub tabs: BTreeMap<String, RestartState>,
    pub notified: bool,
    pub summary_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RunOutcome {
    Running,
    Succeeded,
    Failed(String),
    UnstoppedFailure(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RestartState {
    Pending,
    Claimed,
    Resumed,
    FreshLaunch,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Started,
    Duplicate,
    ProviderBusy,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TabReadiness {
    pub idle: bool,
    pub active_turn: bool,
    pub active_attempt: bool,
}

impl TabReadiness {
    pub fn with_background_work(mut self, running: bool) -> Self {
        self.idle &= !running;
        self
    }
    pub fn can_restart(self) -> bool {
        self.idle && !self.active_turn && !self.active_attempt
    }
}

impl Coordinator {
    pub fn begin(&mut self, provider: &str, current: &str, target: &str) -> Admission {
        let key = format!("{provider}:{target}");
        if self.runs.contains_key(&key) {
            return Admission::Duplicate;
        }
        if self.runs.values().any(|run| {
            run.provider == provider
                && matches!(
                    run.outcome,
                    RunOutcome::Running | RunOutcome::UnstoppedFailure(_)
                )
        }) {
            return Admission::ProviderBusy;
        }
        self.runs.insert(
            key,
            UpdateRun {
                provider: provider.into(),
                current: current.into(),
                target: target.into(),
                outcome: RunOutcome::Running,
                owner_pid: std::process::id(),
                restart_owners: BTreeMap::new(),
                tabs: BTreeMap::new(),
                notified: false,
                summary_id: None,
            },
        );
        Admission::Started
    }

    pub fn enroll(&mut self, key: &str, tabs: &[String]) {
        if let Some(run) = self.runs.get_mut(key) {
            if run.outcome == RunOutcome::Running {
                for tab in tabs {
                    run.tabs.entry(tab.clone()).or_insert(RestartState::Pending);
                }
            }
        }
    }

    pub fn finish(&mut self, key: &str, result: Result<(), String>, tabs: &[String]) {
        self.enroll(key, tabs);
        if let Some(run) = self.runs.get_mut(key) {
            if run.outcome != RunOutcome::Running {
                return;
            }
            run.outcome = match result {
                Ok(()) => RunOutcome::Succeeded,
                Err(error) => {
                    run.tabs.clear();
                    RunOutcome::Failed(error)
                }
            };
        }
    }

    pub fn block_unstopped(&mut self, key: &str) {
        if let Some(run) = self.runs.get_mut(key) {
            if let RunOutcome::Failed(error) = &run.outcome {
                run.outcome = RunOutcome::UnstoppedFailure(error.clone());
            }
        }
    }

    pub fn claim_restart(&mut self, key: &str, tab: &str, readiness: TabReadiness) -> bool {
        let Some(provider) = self.runs.get(key).map(|run| run.provider.clone()) else {
            return false;
        };
        if self.runs.values().any(|run| {
            run.provider == provider
                && matches!(
                    run.outcome,
                    RunOutcome::Running | RunOutcome::UnstoppedFailure(_)
                )
        }) || self
            .runs
            .values()
            .any(|run| run.tabs.get(tab) == Some(&RestartState::Claimed))
        {
            return false;
        }
        let Some(run) = self.runs.get_mut(key) else {
            return false;
        };
        if run.outcome != RunOutcome::Succeeded || !readiness.can_restart() {
            return false;
        }
        let Some(state) = run.tabs.get_mut(tab) else {
            return false;
        };
        if *state != RestartState::Pending {
            return false;
        }
        *state = RestartState::Claimed;
        run.restart_owners.insert(tab.into(), std::process::id());
        true
    }

    pub fn defer_restart(&mut self, key: &str, tab: &str) {
        if let Some(state) = self.runs.get_mut(key).and_then(|run| run.tabs.get_mut(tab)) {
            if *state == RestartState::Claimed {
                *state = RestartState::Pending;
            }
        }
    }

    pub fn settle_restart(&mut self, key: &str, tab: &str, state: RestartState) {
        if let Some(slot) = self.runs.get_mut(key).and_then(|run| run.tabs.get_mut(tab)) {
            if *slot == RestartState::Claimed {
                *slot = state;
            }
        }
    }

    pub fn recover_interrupted(
        &mut self,
        alive: impl Fn(u32) -> bool,
    ) -> Vec<(String, String, Option<String>)> {
        let mut interrupted = Vec::new();
        for run in self.runs.values_mut() {
            if run.outcome == RunOutcome::Running && !alive(run.owner_pid) {
                run.outcome = RunOutcome::UnstoppedFailure("coordinator exited before the update result was recorded; updater outcome and termination are unconfirmed, automatic restarts and further updates are withheld".into());
                run.tabs.clear();
                interrupted.push((run.provider.clone(), run.target.clone(), None));
            }
            for (tab, state) in &mut run.tabs {
                if *state == RestartState::Claimed
                    && run.restart_owners.get(tab).is_some_and(|pid| !alive(*pid))
                {
                    *state = RestartState::Failed;
                    interrupted.push((run.provider.clone(), run.target.clone(), Some(tab.clone())));
                }
            }
        }
        interrupted
    }

    pub fn claim_notice(&mut self, key: &str) -> bool {
        let Some(run) = self.runs.get_mut(key) else {
            return false;
        };
        if run.notified || run.outcome == RunOutcome::Running {
            return false;
        }
        run.notified = true;
        true
    }
}

impl UpdateRun {
    pub fn counts(&self) -> (usize, usize) {
        let restarted = self
            .tabs
            .values()
            .filter(|state| matches!(state, RestartState::Resumed | RestartState::FreshLaunch))
            .count();
        let pending = self
            .tabs
            .values()
            .filter(|state| matches!(state, RestartState::Pending | RestartState::Claimed))
            .count();
        (restarted, pending)
    }
}
