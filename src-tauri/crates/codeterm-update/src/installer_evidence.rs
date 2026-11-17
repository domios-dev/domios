use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub target_version: String,
    pub stage: String,
    pub message: Option<String>,
    pub owner_pid: u32,
    pub updated_at_ms: u64,
    #[serde(skip)]
    data: PathBuf,
}

pub fn path(data: &Path) -> PathBuf {
    data.join("native-update-installer.json")
}

impl Evidence {
    pub fn begin(target: &str) -> Result<Self, String> {
        let evidence = Self {
            target_version: target.into(),
            stage: String::new(),
            message: None,
            owner_pid: std::process::id(),
            updated_at_ms: 0,
            data: codeterm_paths::data_dir(),
        };
        evidence.record("download_verified", None)?;
        Ok(evidence)
    }

    pub fn record(&self, stage: &str, message: Option<&str>) -> Result<(), String> {
        let mut next = self.clone();
        next.stage = stage.into();
        next.message = message.map(str::to_owned);
        next.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let evidence_path = path(&self.data);
        let mut temporary =
            tempfile::NamedTempFile::new_in(&self.data).map_err(|e| e.to_string())?;
        serde_json::to_writer(temporary.as_file_mut(), &next).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary
            .persist(&evidence_path)
            .map_err(|e| e.to_string())?;
        log::info!(target: "native_updater", "installer_stage={} target_version={} owner_pid={} evidence={}", stage, self.target_version, self.owner_pid, evidence_path.display());
        #[cfg(windows)]
        log::info!(target: "native_updater", "native_installer_log={}", std::env::temp_dir().join("Domios-installer.log").display());
        Ok(())
    }

    pub async fn record_async(&self, stage: &str, message: Option<&str>) -> Result<(), String> {
        let evidence = self.clone();
        let stage = stage.to_owned();
        let message = message.map(str::to_owned);
        tokio::task::spawn_blocking(move || evidence.record(&stage, message.as_deref()))
            .await
            .map_err(|e| e.to_string())?
    }
}

pub fn diagnostic(data: &Path, target: Option<&str>) -> String {
    use std::io::Read;
    let evidence_path = path(data);
    let mut bytes = Vec::new();
    let evidence = std::fs::File::open(&evidence_path)
        .ok()
        .and_then(|file| file.take(16385).read_to_end(&mut bytes).ok())
        .filter(|_| bytes.len() <= 16384)
        .and_then(|_| serde_json::from_slice::<Evidence>(&bytes).ok())
        .filter(|evidence| Some(evidence.target_version.as_str()) == target);
    match evidence {
        Some(evidence) => format!(
            "Last installer stage: {}. {} Evidence: {}",
            evidence.stage,
            evidence.message.as_deref().unwrap_or(""),
            evidence_path.display()
        ),
        None => format!(
            "No matching installer launch evidence. Evidence: {}",
            evidence_path.display()
        ),
    }
}

#[cfg(test)]
mod tests;
