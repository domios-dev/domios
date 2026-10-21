//! Loader and writer for persisted state files.
//!
//! An unreadable or unparseable file is never treated as empty: it is moved
//! aside as `<name>.corrupt-<timestamp>`, reported once (ERROR log plus an
//! ActionNeeded notification), and the state is recovered from the newest
//! valid rotating backup. Without a backup the path is held: every save is
//! refused until the file loads again or the hold is explicitly released.

mod incidents;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use incidents::{held, incidents_for, release_hold, set_incident_sink, Hold, Incident};

pub const DEFAULT_BACKUPS: usize = 10;
pub const MAX_CORRUPT_COPIES: usize = 5;
const BACKUP_DIR: &str = "state-backups";

pub type Validator = fn(&str) -> Result<(), String>;

#[derive(Debug, Clone)]
pub struct StateFile {
    path: PathBuf,
    backups: usize,
    validate: Option<Validator>,
}

#[derive(Debug)]
pub enum Loaded<T> {
    Missing,
    Parsed(T),
    /// The original failed to load and the value came from `backup`. When the
    /// original could not be moved aside the path stays held.
    Recovered {
        value: T,
        backup: PathBuf,
    },
    /// No valid backup: the path is held while the caller recovers in memory.
    /// `raw` is the unparseable text when it could be read.
    Unrecoverable {
        raw: Option<String>,
    },
}

impl StateFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            backups: DEFAULT_BACKUPS,
            validate: None,
        }
    }

    pub fn without_backups(mut self) -> Self {
        self.backups = 0;
        self
    }

    /// Checked before every save: an existing file that fails it is preserved
    /// as a corrupt copy instead of being silently replaced.
    pub fn with_validator(mut self, validate: Validator) -> Self {
        self.validate = Some(validate);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn backup_dir(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        parent.join(BACKUP_DIR).join(self.file_name())
    }

    pub fn held(&self) -> Option<Hold> {
        incidents::held(&self.path)
    }

    pub fn hold_for_repair(&self) {
        incidents::hold_for_repair(&self.path, self.corrupt_copies().last().cloned());
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "state".to_string())
    }

    pub fn load<T>(&self, parse: impl Fn(&str) -> Result<T, String>) -> Loaded<T> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
            Err(error) => return self.recover(None, format!("read failed: {error}"), &parse),
        };
        match parse(&text) {
            Ok(value) => {
                incidents::release_hold(&self.path);
                Loaded::Parsed(value)
            }
            Err(error) => self.recover(Some(text), error, &parse),
        }
    }

    fn recover<T>(
        &self,
        raw: Option<String>,
        error: String,
        parse: &impl Fn(&str) -> Result<T, String>,
    ) -> Loaded<T> {
        let preserved = self.quarantine();
        let recovered = self.newest_valid_backup(parse);
        if let (Ok(_), Some((_, backup, bytes))) = (&preserved, &recovered) {
            if let Err(restore_error) = write_atomic(&self.path, bytes) {
                log::warn!(
                    "[state] could not restore {} from {}: {restore_error}",
                    self.path.display(),
                    backup.display()
                );
            }
        }
        let hold = preserved.is_err() || recovered.is_none();
        let incident = Incident {
            path: self.path.clone(),
            error,
            preserved: preserved.as_ref().ok().cloned(),
            preserve_error: preserved.as_ref().err().cloned(),
            recovered_from: recovered.as_ref().map(|(_, backup, _)| backup.clone()),
            held: hold,
        };
        incidents::report(incident);
        match recovered {
            Some((value, backup, _)) => Loaded::Recovered { value, backup },
            None => Loaded::Unrecoverable { raw },
        }
    }

    /// Atomic write (temp file plus rename) followed by a rotating backup.
    /// Refused while the path is held.
    pub fn save(&self, contents: &str) -> Result<(), String> {
        if let Some(hold) = self.held() {
            return Err(hold.refusal());
        }
        self.preserve_invalid_existing()?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        write_atomic(&self.path, contents.as_bytes())
            .map_err(|error| format!("write {}: {error}", self.path.display()))?;
        if self.backups > 0 {
            if let Err(error) = self.write_backup(contents.as_bytes()) {
                log::warn!("[state] backup of {} failed: {error}", self.path.display());
            }
        }
        Ok(())
    }

    pub fn create_held_recovery(&self, contents: &str) -> Result<(), String> {
        self.require_held()?;
        if self.path.exists() {
            return Err(format!("{} already exists", self.path.display()));
        }
        self.write_held_recovery(contents)
    }

    pub fn replace_held_recovery(&self, contents: &str) -> Result<(), String> {
        self.require_held()?;
        let existing = std::fs::read_to_string(&self.path)
            .map_err(|error| format!("read {}: {error}", self.path.display()))?;
        self.validate_contents(&existing)?;
        self.write_held_recovery(contents)
    }

    fn require_held(&self) -> Result<(), String> {
        self.held()
            .map(|_| ())
            .ok_or_else(|| format!("{} is not held", self.path.display()))
    }

    fn write_held_recovery(&self, contents: &str) -> Result<(), String> {
        self.validate_contents(contents)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        write_atomic(&self.path, contents.as_bytes())
            .map_err(|error| format!("write {}: {error}", self.path.display()))
    }

    fn validate_contents(&self, contents: &str) -> Result<(), String> {
        if let Some(validate) = self.validate {
            validate(contents)?;
        }
        Ok(())
    }

    fn preserve_invalid_existing(&self) -> Result<(), String> {
        let Some(validate) = self.validate else {
            return Ok(());
        };
        let error = match std::fs::read_to_string(&self.path) {
            Ok(text) => match validate(&text) {
                Ok(()) => return Ok(()),
                Err(error) => error,
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => format!("read failed: {error}"),
        };
        let preserved = self.quarantine();
        let hold = preserved.is_err();
        let refusal = preserved.as_ref().err().cloned();
        incidents::report(Incident {
            path: self.path.clone(),
            error,
            preserved: preserved.as_ref().ok().cloned(),
            preserve_error: refusal.clone(),
            recovered_from: None,
            held: hold,
        });
        match refusal {
            Some(reason) => Err(format!(
                "refusing to overwrite {}: it fails to load and could not be preserved ({reason})",
                self.path.display()
            )),
            None => Ok(()),
        }
    }

    /// Rename the original to `<name>.corrupt-<timestamp>`, keeping the newest
    /// [`MAX_CORRUPT_COPIES`].
    fn quarantine(&self) -> Result<PathBuf, String> {
        let target = self.corrupt_target();
        std::fs::rename(&self.path, &target).map_err(|error| error.to_string())?;
        self.evict_corrupt_copies();
        Ok(target)
    }

    fn corrupt_target(&self) -> PathBuf {
        let base = format!("{}.corrupt-{:013}", self.file_name(), unix_millis());
        let suffixed = format!("{base}-");
        // Continue after the newest same-millisecond copy, even an evicted base,
        // so name order stays age order.
        let newest_suffix = self
            .corrupt_copies()
            .iter()
            .filter_map(|copy| copy.file_name()?.to_str().map(str::to_string))
            .filter_map(|name| {
                if name == base {
                    Some(0)
                } else {
                    name.strip_prefix(&suffixed)?.parse::<u32>().ok()
                }
            })
            .max();
        match newest_suffix {
            None => self.path.with_file_name(&base),
            Some(newest) => self
                .path
                .with_file_name(format!("{base}-{:03}", newest + 1)),
        }
    }

    pub fn corrupt_copies(&self) -> Vec<PathBuf> {
        let prefix = format!("{}.corrupt-", self.file_name());
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        sorted_entries(dir, |name| name.starts_with(&prefix))
    }

    fn evict_corrupt_copies(&self) {
        let copies = self.corrupt_copies();
        let excess = copies.len().saturating_sub(MAX_CORRUPT_COPIES);
        for stale in copies.into_iter().take(excess) {
            let _ = std::fs::remove_file(stale);
        }
    }

    /// Backups oldest first.
    pub fn backups(&self) -> Vec<PathBuf> {
        sorted_entries(&self.backup_dir(), |name| name.ends_with(".bak"))
    }

    fn newest_valid_backup<T>(
        &self,
        parse: &impl Fn(&str) -> Result<T, String>,
    ) -> Option<(T, PathBuf, Vec<u8>)> {
        self.backups().into_iter().rev().find_map(|backup| {
            let text = std::fs::read_to_string(&backup).ok()?;
            match parse(&text) {
                Ok(value) => Some((value, backup, text.into_bytes())),
                Err(error) => {
                    log::warn!(
                        "[state] skipping invalid backup {}: {error}",
                        backup.display()
                    );
                    None
                }
            }
        })
    }

    fn write_backup(&self, bytes: &[u8]) -> std::io::Result<()> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir)?;
        let existing = self.backups();
        if let Some(newest) = existing.last() {
            if std::fs::read(newest).is_ok_and(|previous| previous == bytes) {
                return Ok(());
            }
        }
        let name = format!("{:013}-{:06}.bak", unix_millis(), next_seq() % 1_000_000);
        write_atomic(&dir.join(name), bytes)?;
        let backups = self.backups();
        let excess = backups.len().saturating_sub(self.backups);
        for stale in backups.into_iter().take(excess) {
            let _ = std::fs::remove_file(stale);
        }
        Ok(())
    }
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

fn next_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

fn sorted_entries(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| keep(entry.file_name().to_string_lossy().as_ref()))
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    entries
}

/// Crash-safe write: a sibling temp file renamed over the target, so a reader
/// sees the old or the new file whole, never a torn write.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "state".to_string());
    let tmp = path.with_file_name(format!("{name}.tmp.{}.{}", std::process::id(), next_seq()));
    std::fs::write(&tmp, bytes)?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}
