use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Coordinator {
    pub runs: BTreeMap<String, UpdateRun>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct UpdateRun {
    pub provider: String,
    pub current: String,
    pub target: String,
    #[serde(default)]
    pub started_at: u64,
    pub outcome: RunOutcome,
    pub owner_pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RunOutcome {
    Running,
    Succeeded,
    Failed(String),
    UnstoppedFailure(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Started,
    Duplicate,
    ProviderBusy,
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
                started_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                outcome: RunOutcome::Running,
                owner_pid: std::process::id(),
            },
        );
        Admission::Started
    }

    pub fn finish(&mut self, key: &str, result: Result<(), String>) {
        if let Some(run) = self.runs.get_mut(key) {
            if run.outcome != RunOutcome::Running {
                return;
            }
            run.outcome = match result {
                Ok(()) => RunOutcome::Succeeded,
                Err(error) => RunOutcome::Failed(error),
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

    pub fn recover_interrupted(&mut self, alive: impl Fn(u32) -> bool) {
        for run in self.runs.values_mut() {
            if run.outcome == RunOutcome::Running && !alive(run.owner_pid) {
                run.outcome = RunOutcome::UnstoppedFailure("coordinator exited before the update result was recorded; updater termination is unconfirmed, further updates are withheld".into());
            }
        }
    }
}
