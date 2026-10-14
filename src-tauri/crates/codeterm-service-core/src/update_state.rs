//! Update state shared by startup, the updater and prompt resolution.

use serde::{Deserialize, Serialize};

use codeterm_types::update::channel::Channel;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

pub(crate) type StatusEmitter = Box<dyn Fn(serde_json::Value) + Send + Sync>;

pub(crate) static STATUS_EMITTER: std::sync::OnceLock<StatusEmitter> = std::sync::OnceLock::new();

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
            "channel": self.channel.to_string(),
            "install_method": self.install_method.to_string(),
            "current_version": env!("CARGO_PKG_VERSION"),
            "latest_known": self.last_check.as_ref().map(|i| i.latest_version.clone()),
            "update_available": self.last_check.as_ref().map(|i| i.update_available).unwrap_or(false),
            "phase_age_secs": self.phase_changed_at.map(|t| t.elapsed().as_secs()),
            "downloaded_bytes": self.downloaded_bytes,
            "download_total_bytes": self.download_total_bytes,
            "download_progress": download_progress,
        })
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
mod state_tests {
    use super::*;

    thread_local! {
        static CAPTURED: std::cell::RefCell<Option<Vec<serde_json::Value>>> = const { std::cell::RefCell::new(None) };
    }

    pub(super) fn capture_status(status: &serde_json::Value) {
        CAPTURED.with_borrow_mut(|events| {
            if let Some(events) = events {
                events.push(status.clone());
            }
        });
    }

    #[test]
    fn update_status_events_follow_state_transitions() {
        CAPTURED.set(Some(Vec::new()));
        let mut state = empty_state();
        state.set_phase(UpdatePhase::Downloading);
        state.record_download_chunk(25, Some(100));
        state.set_error("fixture-failure");
        let events = CAPTURED.take().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["phase"], "downloading");
        assert_eq!(events[1]["downloaded_bytes"], 25);
        assert_eq!(events[1]["download_progress"], 0.25);
        assert_eq!(events[2]["phase"], "error");
        assert_eq!(events[2]["last_error"], "fixture-failure");
    }

    fn empty_state() -> UpdateState {
        UpdateState {
            install_method: InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: Channel::Stable,
            phase: UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        }
    }

    #[test]
    fn set_error_marks_error_phase() {
        let mut s = empty_state();
        s.set_error("boom");
        assert_eq!(s.phase, UpdatePhase::Error);
        assert_eq!(s.last_error.as_deref(), Some("boom"));
        assert!(s.phase_changed_at.is_some());
    }

    #[test]
    fn set_phase_updates_timestamp() {
        let mut s = empty_state();
        s.set_phase(UpdatePhase::Checking);
        assert_eq!(s.phase, UpdatePhase::Checking);
        assert!(s.phase_changed_at.is_some());
    }

    #[test]
    fn clear_error_does_not_change_phase() {
        let mut s = empty_state();
        s.set_error("nope");
        s.clear_error();
        assert!(s.last_error.is_none());
        assert_eq!(s.phase, UpdatePhase::Error);
    }

    #[test]
    fn phase_serializes_snake_case() {
        let v = serde_json::to_string(&UpdatePhase::Downloading).unwrap();
        assert_eq!(v, "\"downloading\"");
    }

    #[test]
    fn download_progress_accumulates_and_caps_at_total() {
        let mut s = empty_state();
        s.set_phase(UpdatePhase::Downloading);
        s.record_download_chunk(40, Some(100));
        s.record_download_chunk(80, Some(100));
        assert_eq!(s.downloaded_bytes, 100);
        assert_eq!(s.download_total_bytes, Some(100));
    }

    #[test]
    fn applying_marks_download_complete_when_total_is_known() {
        let mut s = empty_state();
        s.set_phase(UpdatePhase::Downloading);
        s.record_download_chunk(40, Some(100));
        s.set_phase(UpdatePhase::Applying);
        assert_eq!(s.downloaded_bytes, 100);
    }

    #[test]
    fn runtime_status_reports_download_progress() {
        let mut s = empty_state();
        s.set_phase(UpdatePhase::Downloading);
        s.record_download_chunk(25, Some(100));

        let status = s.runtime_status();
        assert_eq!(status["phase"], "downloading");
        assert_eq!(status["downloaded_bytes"], 25);
        assert_eq!(status["download_total_bytes"], 100);
        assert_eq!(status["download_progress"], 0.25);
    }
}

/// Detect how CodeTerm was installed based on executable path.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
