//! Update state shared by startup, the updater and prompt resolution.

use std::collections::BTreeMap;

use codeterm_plugins::AvailabilityItem;
use serde::{Deserialize, Serialize};

use codeterm_types::update::channel::Channel;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Brew,
    Scoop,
    Dmg,
    Nsis,
    Msi,
    Deb,
    Rpm,
    AppImage,
    Unknown,
}

impl std::fmt::Display for InstallMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Brew => write!(f, "brew"),
            Self::Scoop => write!(f, "scoop"),
            Self::Dmg => write!(f, "dmg"),
            Self::Nsis => write!(f, "nsis"),
            Self::Msi => write!(f, "msi"),
            Self::Deb => write!(f, "deb"),
            Self::Rpm => write!(f, "rpm"),
            Self::AppImage => write!(f, "appimage"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Downloading,
    Applying,
    Success,
    Error,
}

impl std::fmt::Display for UpdatePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Checking => write!(f, "checking"),
            Self::Downloading => write!(f, "downloading"),
            Self::Applying => write!(f, "applying"),
            Self::Success => write!(f, "success"),
            Self::Error => write!(f, "error"),
        }
    }
}

pub struct UpdateState {
    pub install_method: InstallMethod,
    pub last_check: Option<UpdateInfo>,
    pub last_check_time: Option<std::time::Instant>,
    pub channel: Channel,
    pub phase: UpdatePhase,
    pub last_error: Option<String>,
    pub phase_changed_at: Option<std::time::Instant>,
    pub downloaded_bytes: u64,
    pub download_total_bytes: Option<u64>,
}

pub type StatusEmitter = Box<dyn Fn(serde_json::Value) + Send + Sync>;

pub static STATUS_EMITTER: std::sync::OnceLock<StatusEmitter> = std::sync::OnceLock::new();

impl UpdateState {
    fn publish_status(&self) {
        let status = self.runtime_status();
        #[cfg(test)]
        state_tests::capture_status(&status);
        if let Some(emit) = STATUS_EMITTER.get() {
            emit(status);
        }
    }

    pub fn runtime_status(&self) -> serde_json::Value {
        let download_progress = self.download_total_bytes.and_then(|total| {
            if total == 0 {
                None
            } else {
                Some((self.downloaded_bytes as f64 / total as f64).min(1.0))
            }
        });
        serde_json::json!({
            "phase": self.phase.to_string(),
            "last_error": self.last_error,
            "error_revision": self.error_revision(),
            "channel": self.channel.to_string(),
            "install_method": self.install_method.to_string(),
            "current_version": codeterm_build_info::version(),
            "latest_known": self.last_check.as_ref().map(|i| i.latest_version.clone()),
            "update_available": self.last_check.as_ref().map(|i| i.update_available).unwrap_or(false),
            "check_path": self.last_check.as_ref().and_then(|i| i.check_path.clone()),
            "phase_age_secs": self.phase_changed_at.map(|t| t.elapsed().as_secs()),
            "downloaded_bytes": self.downloaded_bytes,
            "download_total_bytes": self.download_total_bytes,
            "download_progress": download_progress,
        })
    }

    pub fn error_revision(&self) -> Option<String> {
        self.last_error.as_ref()?;
        self.phase_changed_at.map(|at| format!("{at:?}"))
    }

    pub fn retire_error(&mut self, revision: Option<&str>) -> bool {
        if self.phase != UpdatePhase::Error
            || revision.is_none()
            || self.error_revision().as_deref() != revision
        {
            return false;
        }
        self.last_error = None;
        self.set_phase(UpdatePhase::Idle);
        true
    }

    pub fn set_phase(&mut self, phase: UpdatePhase) {
        match phase {
            UpdatePhase::Checking | UpdatePhase::Downloading => self.reset_download_progress(),
            UpdatePhase::Applying | UpdatePhase::Success => {
                if let Some(total) = self.download_total_bytes {
                    self.downloaded_bytes = total;
                }
            }
            UpdatePhase::Idle | UpdatePhase::Error => {}
        }
        self.phase = phase;
        self.phase_changed_at = Some(std::time::Instant::now());
        self.publish_status();
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
        self.phase = UpdatePhase::Error;
        self.phase_changed_at = Some(std::time::Instant::now());
        self.publish_status();
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
        self.publish_status();
    }

    pub fn record_download_chunk(&mut self, chunk_len: u64, total: Option<u64>) {
        self.phase = UpdatePhase::Downloading;
        if self.phase_changed_at.is_none() {
            self.phase_changed_at = Some(std::time::Instant::now());
        }
        if total.is_some() {
            self.download_total_bytes = total;
        }
        self.downloaded_bytes = self.downloaded_bytes.saturating_add(chunk_len);
        if let Some(total) = self.download_total_bytes {
            self.downloaded_bytes = self.downloaded_bytes.min(total);
        }
        self.publish_status();
    }

    fn reset_download_progress(&mut self) {
        self.downloaded_bytes = 0;
        self.download_total_bytes = None;
    }
}

#[cfg(test)]
mod state_tests;

/// Detect how Domios was installed based on executable path.
pub fn detect_install_method() -> InstallMethod {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return InstallMethod::Unknown,
    };
    let path = exe.to_string_lossy();

    #[cfg(target_os = "macos")]
    {
        if path.contains("/opt/homebrew/") || path.contains("/usr/local/Caskroom/") {
            return InstallMethod::Brew;
        }
        if path.contains("/Applications/") {
            return InstallMethod::Dmg;
        }
    }

    #[cfg(target_os = "windows")]
    {
        let path_lower = path.to_lowercase();
        if path_lower.contains(r"\scoop\apps\") {
            return InstallMethod::Scoop;
        }
        if path_lower.contains(r"\program files\") || path_lower.contains(r"\program files (x86)\")
        {
            return InstallMethod::Msi;
        }
        return InstallMethod::Nsis;
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("APPIMAGE").is_ok() {
            return InstallMethod::AppImage;
        }
        if path.starts_with("/usr/") {
            if std::path::Path::new("/usr/bin/dpkg").exists() {
                return InstallMethod::Deb;
            }
            if std::path::Path::new("/usr/bin/rpm").exists() {
                return InstallMethod::Rpm;
            }
        }
    }

    #[allow(unreachable_code)]
    InstallMethod::Unknown
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct UpdateInfo {
    pub availability: codeterm_plugins::AvailabilityItem,
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub install_method: InstallMethod,
    /// Command to run for package manager installs
    pub update_command: Option<String>,
    /// Direct download URL for the platform-specific asset
    pub download_url: Option<String>,
    /// GitHub release page URL
    pub release_url: String,
    /// Release notes (markdown body)
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    /// Which path served this check: `direct` or `github_api`; absent when none was needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub check_path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AvailabilitySnapshot {
    #[serde(default)]
    pub checked_at_ms: Option<u64>,
    pub items: Vec<AvailabilityItem>,
    pub errors: BTreeMap<String, String>,
    /// Which path served each source that reached the network, keyed like `errors`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub served_by: BTreeMap<String, String>,
}
