use super::channel::Channel;
use super::{InstallMethod, UpdatePhase, UpdateState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    NoUpdate,
    Restarting,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebianElevationStatus {
    Ready,
    ElevationRequired,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DebianPreflight {
    pub status: DebianElevationStatus,
    pub reason: Option<String>,
}

pub enum DebianUpdatePlan {
    NoUpdate,
    Staged {
        version: String,
        path: std::path::PathBuf,
    },
}

pub enum DebianStageGate {
    NoUpdate,
    Ready {
        version: String,
        path: std::path::PathBuf,
    },
    NotReady(DebianPreflight),
}

#[derive(Clone)]
pub struct ApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
    pub install_method: InstallMethod,
    /// Tab that initiated the update (the GA/caller running `update install`).
    /// Excluded from the shutdown sweep so it survives to hand off (Defect 3).
    pub initiating_tab_id: Option<String>,
}

pub fn begin_apply(update_state: &std::sync::Arc<parking_lot::Mutex<UpdateState>>) {
    let mut us = update_state.lock();
    us.set_phase(UpdatePhase::Checking);
    us.clear_error();
}

pub async fn gate_staged_deb_async<F>(
    plan: DebianUpdatePlan,
    preflight: F,
) -> Result<DebianStageGate, String>
where
    F: FnOnce() -> DebianPreflight + Send + 'static,
{
    match plan {
        DebianUpdatePlan::NoUpdate => Ok(DebianStageGate::NoUpdate),
        DebianUpdatePlan::Staged { version, path } => {
            let status = tokio::task::spawn_blocking(preflight)
                .await
                .map_err(|e| format!("elevation preflight worker failed: {}", e))?;
            if status.status == DebianElevationStatus::Ready {
                Ok(DebianStageGate::Ready { version, path })
            } else {
                Ok(DebianStageGate::NotReady(status))
            }
        }
    }
}

#[cfg(test)]
mod update_apply_phase_tests {
    use super::*;
    use std::sync::Arc;

    fn state() -> Arc<parking_lot::Mutex<UpdateState>> {
        Arc::new(parking_lot::Mutex::new(UpdateState {
            install_method: InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: Channel::Stable,
            phase: UpdatePhase::Idle,
            last_error: Some("stale error".to_string()),
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        }))
    }

    #[test]
    fn update_apply_phase_begins_checking_and_clears_error() {
        let update_state = state();

        begin_apply(&update_state);

        let state = update_state.lock();
        assert_eq!(state.phase, UpdatePhase::Checking);
        assert!(state.last_error.is_none());
    }
}
