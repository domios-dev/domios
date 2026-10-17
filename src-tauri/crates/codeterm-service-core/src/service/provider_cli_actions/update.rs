use codeterm_plugin::provider_cli::ProviderCliUpdateJob;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    LazyLock,
};

static ACTIVE: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static JOBS: LazyLock<Mutex<HashMap<String, ProviderCliUpdateJob>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static REVISION: AtomicU64 = AtomicU64::new(0);

pub struct Permit(String);

impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.lock().remove(&self.0);
    }
}

pub fn reserve(provider: &str) -> Result<Permit, String> {
    if !ACTIVE.lock().insert(provider.to_owned()) {
        return Err("A CLI update is already running for this provider".into());
    }
    Ok(Permit(provider.to_owned()))
}

pub fn record(provider: &str, job: ProviderCliUpdateJob) {
    JOBS.lock().insert(provider.to_owned(), job);
    REVISION.fetch_add(1, Ordering::Release);
}

pub fn job(provider: &str) -> Option<ProviderCliUpdateJob> {
    JOBS.lock().get(provider).cloned()
}
pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

pub fn verified(exit_code: Option<i32>, expected: Option<&str>, actual: Option<&str>) -> bool {
    exit_code == Some(0) && expected.is_some() && expected == actual
}

pub fn windows_update_path(system_root: &str, path: &str) -> String {
    format!(
        "{}\\System32;{path}",
        system_root.trim_end_matches(['\\', '/'])
    )
}

pub fn prepare_command(command: String, platform: &str) -> Result<String, String> {
    if platform != "windows" {
        return Ok(command);
    }
    let root = std::env::var("SystemRoot")
        .map_err(|_| "SystemRoot is unavailable for the CLI update job".to_string())?;
    let prefix = windows_update_path(&root, "").replace('\'', "''");
    // The utility shell and installer children must prefer Windows tar over Git's GNU tar.
    Ok(format!("$env:Path = '{prefix}' + $env:Path; {command}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_update_path_prefers_system_tar_and_preserves_inherited_entries() {
        let inherited = r"C:\Program Files\Git\usr\bin;C:\Windows\System32;C:\custom";
        assert_eq!(
            super::windows_update_path(r"C:\Windows\", inherited),
            format!("C:\\Windows\\System32;{inherited}")
        );
        assert_eq!(
            super::windows_update_path("D:/Windows/", ""),
            "D:/Windows\\System32;"
        );
    }

    #[test]
    fn success_requires_exit_and_exact_installed_version() {
        assert!(super::verified(Some(0), Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(Some(1), Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(None, Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(Some(0), Some("1.2.3"), Some("1.2.2")));
        assert!(!super::verified(Some(0), None, None));
    }
}
