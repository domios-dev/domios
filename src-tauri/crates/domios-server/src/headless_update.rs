#[cfg(target_os = "linux")]
use codeterm_update::headless::{self, Backend, Error, ErrorCode};
use std::io;
#[cfg(any(target_os = "linux", test))]
use std::path::Path;

#[cfg(target_os = "linux")]
mod helper;
pub mod staging;

pub fn run_helper(recover: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        helper::run(recover).map_err(io::Error::other)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recover;
        Err(io::Error::other("Headless update helper requires Linux"))
    }
}

#[cfg(any(target_os = "linux", test))]
fn managed(cgroup: &str, executable: &Path, installed: &Path) -> bool {
    cgroup
        .lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .any(|path| path == "/system.slice/domios-server.service")
        && executable == installed
}

pub fn register_if_managed() {
    if codeterm_update::container::refusal().is_some() {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let evidence = std::fs::read_to_string("/proc/self/cgroup").and_then(|cgroup| {
            Ok((
                cgroup,
                std::env::current_exe()?.canonicalize()?,
                Path::new("/opt/domios/domios-server/domios-server").canonicalize()?,
            ))
        });
        if evidence
            .is_ok_and(|(cgroup, executable, installed)| managed(&cgroup, &executable, &installed))
        {
            let _ = headless::register(std::sync::Arc::new(SystemdBackend));
        }
    }
}

#[cfg(target_os = "linux")]
struct SystemdBackend;

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl Backend for SystemdBackend {
    async fn check(&self) -> Result<codeterm_update::update_state::UpdateInfo, Error> {
        let offer = headless::fetch().await?;
        let manifest = offer.manifest();
        let current = codeterm_build_info::version();
        let available = offer.target_version(current)?.is_some();
        let mut availability = codeterm_plugins::AvailabilityItem::release(
            "app",
            codeterm_plugins::UpdateKind::App,
            current,
            &manifest.version,
            true,
        );
        availability.reason = "signed_headless_feed".into();
        if available {
            availability.state = codeterm_plugins::AvailabilityState::UpdateAvailable;
        }
        Ok(codeterm_update::update_state::UpdateInfo {
            current_version: current.into(),
            latest_version: manifest.version.clone(),
            update_available: available,
            install_method: codeterm_update::InstallMethod::Unknown,
            update_command: None,
            download_url: Some(manifest.url.clone()),
            release_url: format!(
                "https://github.com/domios-dev/domios/releases/tag/v{}",
                manifest.version
            ),
            release_notes: None,
            published_at: None,
            availability,
        })
    }

    async fn install(&self) -> Result<serde_json::Value, Error> {
        let offer = headless::fetch().await?;
        if offer
            .target_version(codeterm_build_info::version())?
            .is_none()
        {
            return Ok(serde_json::json!({"status":"up_to_date"}));
        }
        let _lease = codeterm_update::single_flight::global()
            .try_acquire()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::HelperUnavailable,
                    "An update request is already in flight",
                )
            })?;
        let status = tokio::process::Command::new("/usr/bin/systemctl")
            .args(["start", "--no-block", "domios-update.service"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| Error::new(ErrorCode::HelperUnavailable, e))?;
        if !status.status.success() {
            return Err(Error::new(
                ErrorCode::HelperUnavailable,
                "systemd refused to start domios-update.service; install the exact polkit rule",
            ));
        }
        Ok(
            serde_json::json!({"status":"scheduled", "message":"systemd accepted the update job. The root helper independently verifies the signed feed and archive. Read /api/update/status or journalctl -u domios-update.service for the final result."}),
        )
    }

    fn status(&self) -> Option<serde_json::Value> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open("/var/lib/domios-update/result.json")
            .ok()?
            .take(16385)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 16384 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }
}

#[cfg(test)]
mod tests;
