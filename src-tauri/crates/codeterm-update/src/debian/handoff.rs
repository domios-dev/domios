use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{acknowledge_start, entry, launch, rollback_leases};

#[derive(Clone, Serialize, Deserialize)]
pub struct QuitTarget {
    pub ready: codeterm_types::server_bootstrap::ServerReady,
    pub client: codeterm_types::server_bootstrap::ProcessProvenance,
    pub instance_locks: Vec<std::path::PathBuf>,
    #[serde(skip)]
    pub token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Elevation {
    Pkexec,
    SudoNonInteractive,
}

pub fn elevation(graphical: bool, pkexec: bool, sudo: bool) -> Result<Elevation, String> {
    if graphical && pkexec {
        Ok(Elevation::Pkexec)
    } else if sudo {
        Ok(Elevation::SudoNonInteractive)
    } else {
        Err("No graphical polkit agent or noninteractive sudo is available".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Verify,
    Pin,
    Quit,
    Wait,
    Install,
    Relaunch,
    Cleanup,
}

pub const STEPS: [Step; 7] = [
    Step::Verify,
    Step::Pin,
    Step::Quit,
    Step::Wait,
    Step::Install,
    Step::Relaunch,
    Step::Cleanup,
];

#[derive(Debug, PartialEq, Eq)]
pub enum Relaunch {
    Service,
    Installed,
    OldSnapshot,
}

pub fn relaunch_target(rollback: bool, unit_exists: bool) -> Relaunch {
    if rollback {
        Relaunch::OldSnapshot
    } else if unit_exists {
        Relaunch::Service
    } else {
        Relaunch::Installed
    }
}

pub fn use_systemd(elevation: Elevation, available: bool) -> bool {
    elevation == Elevation::SudoNonInteractive && available
}

pub fn inside_systemd_unit(invocation_id: bool, cgroup: &str) -> bool {
    invocation_id
        || cgroup
            .lines()
            .any(|line| line.trim_end().ends_with(".service"))
}

pub fn launch_mode(
    inside_unit: bool,
    graphical: bool,
    pkexec: bool,
    sudo: bool,
    systemd: bool,
) -> Result<(Elevation, bool), String> {
    if inside_unit && (!systemd || !sudo) {
        return Err("Automatic update from a systemd unit requires systemd-run and noninteractive sudo; the session is still running".into());
    }
    let elevation = elevation(graphical && !inside_unit, pkexec, sudo)?;
    Ok((elevation, use_systemd(elevation, systemd)))
}

pub fn may_restore_snapshot(old_pair_exited: bool, recovery_ready: bool) -> bool {
    old_pair_exited && recovery_ready
}

#[async_trait::async_trait]
pub trait Port: Send {
    async fn execute(&mut self, step: Step) -> Result<(), String>;
    async fn recover(&mut self, failure: &str);
}

pub async fn run(port: &mut dyn Port) -> Result<(), String> {
    for step in STEPS {
        if let Err(error) = port.execute(step).await {
            port.recover(&format!("Debian update {step:?} failed: {error}"))
                .await;
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Serialize, Deserialize)]
struct Plan {
    directory: PathBuf,
    deb: PathBuf,
    sha256: String,
    version: String,
    app: PathBuf,
    elevation: Elevation,
    target: QuitTarget,
    snapshot: codeterm_types::app_quit::QuitSnapshot,
}

pub fn exact_pair(
    expected: (
        &codeterm_types::server_bootstrap::ProcessProvenance,
        &codeterm_types::server_bootstrap::ProcessProvenance,
    ),
    actual: (
        &codeterm_types::server_bootstrap::ProcessProvenance,
        &codeterm_types::server_bootstrap::ProcessProvenance,
    ),
) -> bool {
    expected == actual
}

#[cfg(test)]
mod tests;
