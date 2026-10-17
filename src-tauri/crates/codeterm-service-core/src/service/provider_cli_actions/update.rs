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

#[cfg(test)]
mod tests {
    #[test]
    fn success_requires_exit_and_exact_installed_version() {
        assert!(super::verified(Some(0), Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(Some(1), Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(None, Some("1.2.3"), Some("1.2.3")));
        assert!(!super::verified(Some(0), Some("1.2.3"), Some("1.2.2")));
        assert!(!super::verified(Some(0), None, None));
    }
}
