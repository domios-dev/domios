//! Platform-specific upgrade orchestration.
//!
//! The flow is the same on every OS — kill, swap, start, verify, maybe
//! roll back — but the *executor* is OS-specific. The daemon needs to
//! die before the binary can be replaced, so we hand a script to an
//! external dispatcher (schtasks / launchctl / at) that fires AFTER the
//! daemon exits.

use std::path::{Path, PathBuf};

use super::result::UpgradeContext;

pub mod windows;
pub mod macos;
pub mod linux;

/// Inputs collected by the daemon, handed to the platform orchestrator.
pub struct OrchestratorInputs {
    /// Where staging lives (data_dir/upgrade-staging).
    pub staging_dir: PathBuf,
    /// Path to the freshly downloaded NSIS .exe / .dmg / .AppImage.
    pub installer_path: PathBuf,
    /// Path to the currently-running codeterm executable.
    pub current_exe: PathBuf,
    /// API base URL used by the script for the post-swap health check.
    /// Always loopback (http://127.0.0.1:<port>/api/health).
    pub health_url: String,
    /// API token the script must send as `Authorization: Bearer <token>`
    /// when probing /api/health.
    pub api_token: String,
    /// Final destination for `upgrade-result.json` (data_dir/upgrade-result.json).
    pub result_path: PathBuf,
    /// Final log path written by the script.
    pub log_path: PathBuf,
    pub context: UpgradeContext,
}

pub trait Orchestrator {
    /// Generate the platform-specific orchestrator script. Pure: no
    /// filesystem or process side effects, so it's easy to unit-test.
    fn render_script(&self, inputs: &OrchestratorInputs) -> String;

    /// Materialize the script file under `staging_dir` and schedule it to
    /// run AFTER the daemon dies. Returns the path of the script file
    /// actually written.
    fn dispatch(&self, inputs: &OrchestratorInputs) -> Result<PathBuf, String>;
}

#[cfg(target_os = "windows")]
pub fn for_current_os() -> Box<dyn Orchestrator + Send + Sync> {
    Box::new(windows::WindowsOrchestrator)
}

#[cfg(target_os = "macos")]
pub fn for_current_os() -> Box<dyn Orchestrator + Send + Sync> {
    Box::new(macos::MacOsOrchestrator)
}

#[cfg(target_os = "linux")]
pub fn for_current_os() -> Box<dyn Orchestrator + Send + Sync> {
    Box::new(linux::LinuxOrchestrator)
}

/// Produce the on-disk script file path for a given staging dir.
/// Lives in this module so callers don't need to know the per-OS extension.
pub fn script_path(staging_dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        staging_dir.join("run.ps1")
    } else {
        staging_dir.join("run.sh")
    }
}
