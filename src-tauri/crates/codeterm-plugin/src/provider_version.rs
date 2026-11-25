use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
mod child;
use child::ProbeChild;

// WHY: the probe runs on its own thread; npm `.cmd` shims under startup load exceed 3 s.
const PROBE_DEADLINE: Duration = Duration::from_secs(10);
/// A failed probe is retried after this, so one slow start never disables a provider for good.
const RETRY_BACKOFF: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    #[default]
    Pending,
    Detected,
    Missing,
    Unreadable,
    Failed,
    Timeout,
    Invalid,
    Saturated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionObservation {
    pub version: Option<String>,
    pub status: ProbeStatus,
}

impl VersionObservation {
    fn unknown(status: ProbeStatus) -> Self {
        Self {
            version: None,
            status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BinaryIdentity {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    inode: (u64, u64, i64, i64),
}

impl BinaryIdentity {
    fn read(path: &Path) -> Option<Self> {
        let path = path.canonicalize().ok()?;
        let metadata = path.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            path,
            len: metadata.len(),
            modified: metadata.modified().ok()?,
            created: metadata.created().ok(),
            #[cfg(unix)]
            inode: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheEntry {
    observation: VersionObservation,
    settled: Instant,
}

impl CacheEntry {
    fn retry_due(&self, now: Instant) -> bool {
        matches!(
            self.observation.status,
            ProbeStatus::Failed | ProbeStatus::Timeout | ProbeStatus::Invalid
        ) && now.saturating_duration_since(self.settled) >= RETRY_BACKOFF
    }
}

#[derive(Default)]
struct VersionCache {
    entries: HashMap<BinaryIdentity, CacheEntry>,
}

impl VersionCache {
    fn reserve(&mut self, identity: &BinaryIdentity, now: Instant) -> (VersionObservation, bool) {
        if let Some(entry) = self
            .entries
            .get(identity)
            .filter(|entry| !entry.retry_due(now))
        {
            return (entry.observation.clone(), false);
        }
        self.entries.retain(|key, entry| {
            key.path != identity.path || entry.observation.status == ProbeStatus::Pending
        });
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            return (VersionObservation::unknown(ProbeStatus::Saturated), false);
        }
        let pending = VersionObservation::unknown(ProbeStatus::Pending);
        self.settle(identity.clone(), pending.clone(), now);
        (pending, true)
    }

    fn settle(&mut self, identity: BinaryIdentity, observation: VersionObservation, now: Instant) {
        self.entries.insert(
            identity,
            CacheEntry {
                observation,
                settled: now,
            },
        );
    }
}

static CACHE: LazyLock<Mutex<VersionCache>> = LazyLock::new(|| Mutex::new(VersionCache::default()));
static REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

struct PendingSettlement {
    identity: Option<BinaryIdentity>,
}

impl PendingSettlement {
    fn new(identity: BinaryIdentity) -> Self {
        Self {
            identity: Some(identity),
        }
    }

    fn settle(mut self, observed: VersionObservation) {
        let Some(identity) = self.identity.take() else {
            return;
        };
        let unchanged = BinaryIdentity::read(&identity.path).as_ref() == Some(&identity);
        let mut cache = CACHE.lock();
        if unchanged {
            cache.settle(identity, observed, Instant::now());
        } else {
            cache.entries.remove(&identity);
        }
        REVISION.fetch_add(1, Ordering::Release);
    }
}

impl Drop for PendingSettlement {
    fn drop(&mut self) {
        let Some(identity) = self.identity.take() else {
            return;
        };
        let mut cache = CACHE.lock();
        if cache
            .entries
            .get(&identity)
            .is_some_and(|entry| entry.observation.status == ProbeStatus::Pending)
        {
            cache.settle(
                identity,
                VersionObservation::unknown(ProbeStatus::Failed),
                Instant::now(),
            );
            REVISION.fetch_add(1, Ordering::Release);
        }
    }
}

pub fn observe_in(binary: &str, extra_dirs: &[PathBuf]) -> VersionObservation {
    if binary.trim().is_empty() {
        return VersionObservation::unknown(ProbeStatus::Missing);
    }
    let path = match crate::js_runtime::probe_bin_with_extra_dirs(binary, extra_dirs).resolution {
        crate::js_runtime::BinResolution::Found(path) => path,
        crate::js_runtime::BinResolution::Missing => {
            return VersionObservation::unknown(ProbeStatus::Missing)
        }
        crate::js_runtime::BinResolution::Unknown => {
            return VersionObservation::unknown(ProbeStatus::Unreadable)
        }
    };
    let Some(identity) = BinaryIdentity::read(&path) else {
        return VersionObservation::unknown(ProbeStatus::Unreadable);
    };
    let (observation, start) = CACHE.lock().reserve(&identity, Instant::now());
    if start {
        let reserved = identity.clone();
        let spawned = std::thread::Builder::new()
            .name("provider-version".into())
            .spawn(move || {
                let settlement = PendingSettlement::new(identity);
                let observed = probe(&path);
                log::debug!(
                    "[provider-version] path={} status={:?} version={:?}",
                    path.display(),
                    observed.status,
                    observed.version
                );
                settlement.settle(observed);
            });
        if spawned.is_err() {
            CACHE.lock().settle(
                reserved,
                VersionObservation::unknown(ProbeStatus::Failed),
                Instant::now(),
            );
            REVISION.fetch_add(1, Ordering::Release);
            return VersionObservation::unknown(ProbeStatus::Failed);
        }
    }
    observation
}

pub fn refresh(path: &Path) -> VersionObservation {
    let identity = BinaryIdentity::read(path);
    let observed = probe(path);
    if let Some(identity) =
        identity.filter(|identity| BinaryIdentity::read(path).as_ref() == Some(identity))
    {
        let mut cache = CACHE.lock();
        cache.entries.retain(|key, _| key.path != identity.path);
        cache.settle(identity, observed.clone(), Instant::now());
        REVISION.fetch_add(1, Ordering::Release);
    }
    observed
}

fn probe(path: &Path) -> VersionObservation {
    if path.as_os_str().is_empty() || !path.is_file() {
        return VersionObservation::unknown(ProbeStatus::Missing);
    }
    let command = match crate::provider_command::command(path) {
        Ok(command) => command,
        Err(error) => {
            log::debug!("[provider-version] {error}");
            return VersionObservation::unknown(ProbeStatus::Failed);
        }
    };
    probe_command(command)
}

fn probe_command(command: Command) -> VersionObservation {
    probe_command_with_deadline(command, PROBE_DEADLINE)
}

fn probe_command_with_deadline(mut command: Command, timeout: Duration) -> VersionObservation {
    let deadline = Instant::now() + timeout;
    command.arg("--version");
    let Ok(mut child) = ProbeChild::spawn(&mut command) else {
        return VersionObservation::unknown(ProbeStatus::Failed);
    };
    let bytes = match child.collect_output(deadline, MAX_OUTPUT_BYTES as usize) {
        Ok(Some((status, bytes))) if status.success() => bytes,
        Ok(Some(_)) | Err(_) => return VersionObservation::unknown(ProbeStatus::Failed),
        Ok(None) => return VersionObservation::unknown(ProbeStatus::Timeout),
    };
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return VersionObservation::unknown(ProbeStatus::Invalid);
    }
    match String::from_utf8(bytes)
        .ok()
        .and_then(|output| parse_version(&output))
    {
        Some(version) => VersionObservation {
            version: Some(version),
            status: ProbeStatus::Detected,
        },
        None => VersionObservation::unknown(ProbeStatus::Invalid),
    }
}

pub fn parse_version(output: &str) -> Option<String> {
    let versions: Vec<_> = output
        .split_whitespace()
        .filter_map(|token| semver::Version::parse(token.trim_start_matches('v')).ok())
        .collect();
    (versions.len() == 1).then(|| versions[0].to_string())
}

#[cfg(test)]
mod tests;
