use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use wait_timeout::ChildExt;

const PROBE_DEADLINE: Duration = Duration::from_secs(3);
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Default)]
struct VersionCache {
    entries: HashMap<BinaryIdentity, VersionObservation>,
}

impl VersionCache {
    fn reserve(&mut self, identity: &BinaryIdentity) -> (VersionObservation, bool) {
        if let Some(observed) = self.entries.get(identity) {
            return (observed.clone(), false);
        }
        self.entries
            .retain(|key, value| key.path != identity.path || value.status == ProbeStatus::Pending);
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            return (VersionObservation::unknown(ProbeStatus::Saturated), false);
        }
        let pending = VersionObservation::unknown(ProbeStatus::Pending);
        self.entries.insert(identity.clone(), pending.clone());
        (pending, true)
    }
}

static CACHE: LazyLock<Mutex<VersionCache>> = LazyLock::new(|| Mutex::new(VersionCache::default()));

pub fn observe(binary: &str) -> VersionObservation {
    let path = match crate::js_runtime::probe_bin(binary).resolution {
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
    let (observation, start) = CACHE.lock().reserve(&identity);
    if start {
        let reserved = identity.clone();
        let spawned = std::thread::Builder::new()
            .name("provider-version".into())
            .spawn(move || {
                let observed = probe(&identity.path);
                let unchanged = BinaryIdentity::read(&identity.path).as_ref() == Some(&identity);
                let mut cache = CACHE.lock();
                if unchanged {
                    cache.entries.insert(identity, observed);
                } else {
                    cache.entries.remove(&identity);
                }
            });
        if spawned.is_err() {
            CACHE
                .lock()
                .entries
                .insert(reserved, VersionObservation::unknown(ProbeStatus::Failed));
            return VersionObservation::unknown(ProbeStatus::Failed);
        }
    }
    observation
}

fn probe(path: &Path) -> VersionObservation {
    let deadline = Instant::now() + PROBE_DEADLINE;
    let mut command = Command::new(path);
    codeterm_util::process_util::no_window_std(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let Ok(mut child) = command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return VersionObservation::unknown(ProbeStatus::Failed);
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return VersionObservation::unknown(ProbeStatus::Failed);
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("provider-version-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut bytes);
            let _ = sender.send(result.ok().map(|_| bytes));
        });
    let status = if reader.is_ok() {
        child.wait_timeout(deadline.saturating_duration_since(Instant::now()))
    } else {
        Ok(None)
    };
    match status {
        Ok(Some(status)) if status.success() => {}
        other => {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return VersionObservation::unknown(if matches!(other, Ok(None)) {
                ProbeStatus::Timeout
            } else {
                ProbeStatus::Failed
            });
        }
    }
    let Ok(Some(bytes)) = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    else {
        return VersionObservation::unknown(ProbeStatus::Timeout);
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
