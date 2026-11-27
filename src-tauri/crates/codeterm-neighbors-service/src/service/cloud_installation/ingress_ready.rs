use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use codeterm_account::account_mesh::cloud_ingress_ready;
use serde::{Deserialize, Serialize};

use super::{CloudInstallation, Phase};

#[derive(Deserialize, Serialize)]
struct BootIdentity {
    id: String,
    started_at_ms: u64,
}

impl BootIdentity {
    fn load(root: &Path) -> io::Result<Self> {
        let id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        let id = id.trim();
        let path = root.join("cloud-ingress-boot.json");
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(stored) = serde_json::from_slice::<Self>(&bytes) {
                if stored.id == id {
                    return Ok(stored);
                }
            }
        }
        let started_at_ms = fs::read_to_string("/proc/stat")?
            .lines()
            .find_map(|line| line.strip_prefix("btime "))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "kernel btime unavailable")
            })?;
        let boot = Self {
            id: id.to_owned(),
            started_at_ms,
        };
        let bytes = serde_json::to_vec(&boot).map_err(io::Error::other)?;
        codeterm_util::atomic_write::write_atomic(&path, bytes)?;
        Ok(boot)
    }
}

impl CloudInstallation {
    pub async fn record_ingress_ready(&self) -> Result<Option<u64>, String> {
        let account_id = match &*self.0.phase.lock() {
            Phase::Enrolled { account_id, .. } => account_id.clone(),
            _ => return Ok(None),
        };
        let root = self.0.root.clone();
        let boot = tokio::task::spawn_blocking(move || BootIdentity::load(&root))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "wall clock precedes Unix epoch")?;
        cloud_ingress_ready::record(
            self.0.trust.project(),
            &account_id,
            self.installation_id(),
            &self.0.signer,
            &boot.id,
            boot.started_at_ms,
            now.as_secs(),
        )
        .await
        .map_err(|error| format!("{error:?}"))
    }
}
