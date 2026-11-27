use std::time::{SystemTime, UNIX_EPOCH};

use codeterm_account::account_mesh::cloud_ingress_ready;

use super::{CloudInstallation, Phase};

impl CloudInstallation {
    pub async fn record_ingress_ready(&self) -> Result<Option<u64>, String> {
        let account_id = match &*self.0.phase.lock() {
            Phase::Enrolled { account_id, .. } => account_id.clone(),
            _ => return Ok(None),
        };
        let boot_id = tokio::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .await
            .map_err(|_| "kernel boot identifier unavailable")?;
        let mut uptime = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: uptime points to writable timespec storage for the duration of the call.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut uptime) } != 0 {
            return Err("kernel boot clock unavailable".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "wall clock precedes Unix epoch")?;
        let wall_ms = u64::try_from(now.as_millis()).map_err(|_| "wall clock out of range")?;
        let uptime_ms = u64::try_from(uptime.tv_sec)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .and_then(|millis| millis.checked_add(u64::try_from(uptime.tv_nsec).ok()? / 1_000_000))
            .ok_or("kernel boot clock out of range")?;
        let started_ms = wall_ms.checked_sub(uptime_ms).ok_or("invalid boot time")?;
        cloud_ingress_ready::record(
            self.0.trust.project(),
            &account_id,
            self.installation_id(),
            &self.0.signer,
            boot_id.trim(),
            started_ms,
            now.as_secs(),
        )
        .await
        .map_err(|error| format!("{error:?}"))
    }
}
