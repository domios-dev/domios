use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

mod detection;
mod paths;
pub use paths::{windows_to_wsl, wsl_to_windows};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellProfile {
    pub id: String,
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub wsl_distro: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunch {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

impl ShellProfile {
    pub fn launch(&self, cwd: Option<&str>) -> Result<ShellLaunch, String> {
        let mut args = self.args.clone();
        let cwd = match (&self.wsl_distro, cwd) {
            (Some(distro), Some(cwd)) => {
                args.push("--cd".into());
                args.push(windows_to_wsl(cwd, distro)?);
                None
            }
            (Some(_), None) => {
                args.extend(["--cd".into(), "~".into()]);
                None
            }
            (None, cwd) => cwd.map(str::to_owned),
        };
        Ok(ShellLaunch {
            executable: self.executable.clone(),
            args,
            cwd,
        })
    }

    pub fn host_cwd(&self, cwd: &str) -> Result<String, String> {
        match &self.wsl_distro {
            Some(distro) => wsl_to_windows(cwd, distro),
            None => Ok(cwd.to_owned()),
        }
    }
}

#[derive(Default)]
pub struct ShellProfileCatalog {
    profiles: parking_lot::RwLock<Option<Arc<Vec<ShellProfile>>>>,
    detection: parking_lot::Mutex<()>,
}

impl ShellProfileCatalog {
    pub async fn list(&'static self, refresh: bool) -> Result<Arc<Vec<ShellProfile>>, String> {
        if !refresh {
            if let Some(profiles) = self.profiles.read().clone() {
                return Ok(profiles);
            }
        }
        tokio::task::spawn_blocking(move || self.load(refresh))
            .await
            .map_err(|error| format!("Shell detection failed: {error}"))
    }

    fn load(&self, refresh: bool) -> Arc<Vec<ShellProfile>> {
        self.load_with(refresh, detection::detect)
    }

    fn load_with(
        &self,
        refresh: bool,
        detect: impl FnOnce() -> Vec<ShellProfile>,
    ) -> Arc<Vec<ShellProfile>> {
        let _detection = self.detection.lock();
        if !refresh {
            if let Some(profiles) = self.profiles.read().as_ref() {
                return profiles.clone();
            }
        }
        let profiles = Arc::new(detect());
        *self.profiles.write() = Some(profiles.clone());
        profiles
    }
}

pub fn catalog() -> &'static ShellProfileCatalog {
    static CATALOG: OnceLock<ShellProfileCatalog> = OnceLock::new();
    CATALOG.get_or_init(ShellProfileCatalog::default)
}

#[cfg(test)]
mod tests;
