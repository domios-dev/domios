use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MuxEmitCause {
    AgentIdentity,
    AgentRestart,
    BackgroundWork,
    BootStage,
    Capability,
    ContextFeed,
    Cwd,
    GeneralAgent,
    Model,
    PlanTask,
    Prompt,
    ProviderSignal,
    RuntimeSignal,
    SessionBinding,
    SessionRestore,
    Startup,
    Surface,
    TabLifecycle,
    TabMetadata,
    TabStatus,
    Team,
    Title,
    Watcher,
    Workspace,
}

impl MuxEmitCause {
    pub const ALL: [Self; 24] = [
        Self::AgentIdentity,
        Self::AgentRestart,
        Self::BackgroundWork,
        Self::BootStage,
        Self::Capability,
        Self::ContextFeed,
        Self::Cwd,
        Self::GeneralAgent,
        Self::Model,
        Self::PlanTask,
        Self::Prompt,
        Self::ProviderSignal,
        Self::RuntimeSignal,
        Self::SessionBinding,
        Self::SessionRestore,
        Self::Startup,
        Self::Surface,
        Self::TabLifecycle,
        Self::TabMetadata,
        Self::TabStatus,
        Self::Team,
        Self::Title,
        Self::Watcher,
        Self::Workspace,
    ];

    pub(super) fn bit(self) -> u64 {
        1 << self as u8
    }
}

#[derive(Default)]
pub struct MuxEmitCauseStats {
    requests: [AtomicU64; 24],
    fires: [AtomicU64; 24],
}

#[derive(serde::Serialize)]
pub struct MuxEmitCauseCounts {
    pub requests: u64,
    pub fires: u64,
}

impl MuxEmitCauseStats {
    pub(super) fn request(&self, cause: MuxEmitCause) {
        self.requests[cause as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn fire(&self, causes: u64) {
        for cause in MuxEmitCause::ALL {
            if causes & cause.bit() != 0 {
                self.fires[cause as usize].fetch_add(1, Ordering::Relaxed);
                log::debug!("[events] mux-state-changed fire cause={cause:?}");
            }
        }
    }

    pub fn snapshot(&self) -> BTreeMap<MuxEmitCause, MuxEmitCauseCounts> {
        MuxEmitCause::ALL
            .into_iter()
            .map(|cause| {
                (
                    cause,
                    MuxEmitCauseCounts {
                        requests: self.requests[cause as usize].load(Ordering::Relaxed),
                        fires: self.fires[cause as usize].load(Ordering::Relaxed),
                    },
                )
            })
            .collect()
    }
}
