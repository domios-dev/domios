use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::types::{CloudError, CloudLifecycleState, CloudSleepMode};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PeerBusySnapshot {
    pub active_agent_turns: usize,
    pub running_background_jobs: usize,
    pub held_sends: usize,
    pub attached_viewers: usize,
}

impl PeerBusySnapshot {
    pub fn is_idle(self) -> bool {
        self.active_agent_turns == 0
            && self.running_background_jobs == 0
            && self.held_sends == 0
            && self.attached_viewers == 0
    }

    pub fn busy_reason(self) -> Option<String> {
        if self.is_idle() {
            return None;
        }
        let mut parts = Vec::new();
        if self.active_agent_turns > 0 {
            parts.push(format!("active_agent_turns={}", self.active_agent_turns));
        }
        if self.running_background_jobs > 0 {
            parts.push(format!(
                "running_background_jobs={}",
                self.running_background_jobs
            ));
        }
        if self.held_sends > 0 {
            parts.push(format!("held_sends={}", self.held_sends));
        }
        if self.attached_viewers > 0 {
            parts.push(format!("attached_viewers={}", self.attached_viewers));
        }
        Some(parts.join(", "))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleSleepAction {
    StopProvider,
    DeleteForRecreate,
    KeepUnderLease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdlePolicy {
    pub idle_window_ms: u64,
    pub auto_stop_minutes: u32,
    pub sleep_mode: CloudSleepMode,
    pub delete_on_stop_billed: bool,
}

impl IdlePolicy {
    pub fn sleep_action(self) -> IdleSleepAction {
        match self.sleep_mode {
            CloudSleepMode::Pause | CloudSleepMode::Stop => IdleSleepAction::StopProvider,
            CloudSleepMode::DeleteRecreate => IdleSleepAction::DeleteForRecreate,
            CloudSleepMode::StopBilled => {
                if self.delete_on_stop_billed {
                    IdleSleepAction::DeleteForRecreate
                } else {
                    IdleSleepAction::KeepUnderLease
                }
            }
        }
    }

    /// Renew provider activity well before provider auto-stop can fire during active work.
    pub fn renewal_interval_ms(self) -> Option<u64> {
        if self.auto_stop_minutes == 0 {
            return None;
        }
        let auto_stop_ms = u64::from(self.auto_stop_minutes) * 60_000;
        let safety_margin_ms = (auto_stop_ms / 3)
            .max(30_000)
            .min(auto_stop_ms.saturating_sub(10_000));
        Some(auto_stop_ms.saturating_sub(safety_margin_ms).max(10_000))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdleEvalInput {
    pub state: CloudLifecycleState,
    pub busy: PeerBusySnapshot,
    pub now_ms: u64,
    pub idle_since_ms: Option<u64>,
    pub last_activity_refresh_ms: Option<u64>,
    pub policy: IdlePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdleDecision {
    None,
    RenewActivityNow {
        next_check_at_ms: u64,
    },
    BeginTwoPhaseDrain {
        action: IdleSleepAction,
    },
    ScheduleAt {
        at_ms: u64,
        idle_since_ms: Option<u64>,
    },
}

pub fn evaluate_idle_and_renewal(input: IdleEvalInput) -> IdleDecision {
    if input.state != CloudLifecycleState::Awake {
        return IdleDecision::None;
    }

    if !input.busy.is_idle() {
        let Some(interval) = input.policy.renewal_interval_ms() else {
            return IdleDecision::None;
        };
        let last = input.last_activity_refresh_ms.unwrap_or(0);
        let due_at = last.saturating_add(interval);
        if input.now_ms >= due_at {
            return IdleDecision::RenewActivityNow {
                next_check_at_ms: input.now_ms.saturating_add(interval),
            };
        }
        return IdleDecision::ScheduleAt {
            at_ms: due_at,
            idle_since_ms: None,
        };
    }

    let idle_since = input.idle_since_ms.unwrap_or(input.now_ms);
    let sleep_at = idle_since.saturating_add(input.policy.idle_window_ms);
    if input.now_ms >= sleep_at {
        let action = input.policy.sleep_action();
        return match action {
            IdleSleepAction::KeepUnderLease => IdleDecision::None,
            action => IdleDecision::BeginTwoPhaseDrain { action },
        };
    }

    IdleDecision::ScheduleAt {
        at_ms: sleep_at,
        idle_since_ms: Some(idle_since),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DrainToken {
    pub token: String,
    pub entered_at_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum GatePhase {
    #[default]
    Open,
    Draining(DrainToken),
}

#[derive(Default)]
pub struct LocalPeerDrainGate {
    phase: Mutex<GatePhase>,
}

impl LocalPeerDrainGate {
    pub fn try_enter_drain(
        &self,
        machine_id: &str,
        busy: PeerBusySnapshot,
        now_ms: u64,
    ) -> Result<DrainToken, CloudError> {
        let mut phase = self.phase.lock();
        if let Some(reason) = busy.busy_reason() {
            return Err(CloudError::MachineBusy {
                machine_id: machine_id.into(),
                reason,
            });
        }
        match &*phase {
            GatePhase::Draining(existing) => Ok(existing.clone()),
            GatePhase::Open => {
                let token = DrainToken {
                    token: uuid::Uuid::new_v4().to_string(),
                    entered_at_ms: now_ms,
                };
                *phase = GatePhase::Draining(token.clone());
                Ok(token)
            }
        }
    }

    pub fn reopen(&self) {
        *self.phase.lock() = GatePhase::Open;
    }

    pub fn is_draining(&self) -> bool {
        matches!(*self.phase.lock(), GatePhase::Draining(_))
    }

    pub fn check_write_allowed(&self, machine_id: &str) -> Result<(), CloudError> {
        if self.is_draining() {
            return Err(CloudError::MachineSleeping {
                machine_id: machine_id.into(),
            });
        }
        Ok(())
    }
}
