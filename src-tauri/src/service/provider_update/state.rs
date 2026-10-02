use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct Coordinator {
    pub runs: BTreeMap<String, UpdateRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct UpdateRun {
    pub provider: String,
    pub current: String,
    pub target: String,
    pub outcome: RunOutcome,
    pub tabs: BTreeMap<String, RestartState>,
    pub notified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RunOutcome { Running, Succeeded, Failed(String) }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RestartState { Pending, Claimed, Resumed, FreshLaunch, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission { Started, Duplicate, ProviderBusy }
#[derive(Clone, Copy, Debug)]
pub(super) struct TabReadiness { pub idle: bool, pub active_turn: bool, pub active_attempt: bool }

impl TabReadiness {
    pub fn can_restart(self) -> bool { false }
}
impl Coordinator {
    pub fn begin(&mut self, provider: &str, current: &str, target: &str) -> Admission {
        let key = format!("{provider}:{target}");
        if self.runs.contains_key(&key) { return Admission::Duplicate; }
        self.runs.insert(key, UpdateRun { provider: provider.into(), current: current.into(), target: target.into(), outcome: RunOutcome::Running, tabs: BTreeMap::new(), notified: false });
        Admission::Started
    }
    pub fn finish(&mut self, key: &str, result: Result<(), String>, tabs: &[String]) {
        if let Some(run) = self.runs.get_mut(key) { run.outcome = match result { Ok(()) => RunOutcome::Succeeded, Err(error) => RunOutcome::Failed(error) }; }
    }
    pub fn claim_restart(&mut self, key: &str, tab: &str, readiness: TabReadiness) -> bool { false }
    pub fn settle_restart(&mut self, key: &str, tab: &str, state: RestartState) {}
    pub fn claim_notice(&mut self, key: &str) -> bool { false }
}
impl UpdateRun {
    pub fn counts(&self) -> (usize, usize) { (0, 0) }
}
