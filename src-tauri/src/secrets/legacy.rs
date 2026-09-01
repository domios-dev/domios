//! Erasure of the retired `~/.codeterm/secrets/bitwarden-master.env` drop.
//! No backend reads it, so every copy is a master password at rest with no consumer.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const LEGACY_MASTER_PASSWORD_FILE: &str = "bitwarden-master.env";

/// What the filesystem actually proved. Only `SecurelyErased` may be reported as
/// a secure delete; every other variant is degraded and must be logged as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyPurge {
    /// Nothing to erase.
    Absent,
    /// Bytes overwritten and synced, then the entry unlinked.
    SecurelyErased,
    /// The entry is gone but the bytes were never overwritten, so the old
    /// contents may still be recoverable from the freed blocks.
    RemovedWithoutOverwrite,
    /// The entry survives. `overwritten` says whether the bytes were zeroed first.
    RemovalFailed { overwritten: bool },
    /// The path could not be inspected. Not the same as absent.
    InspectionFailed,
}

impl LegacyPurge {
    pub fn is_secure(&self) -> bool {
        matches!(self, LegacyPurge::SecurelyErased)
    }

    /// Whether the operator needs to hear about this outcome.
    pub fn is_degraded(&self) -> bool {
        !matches!(self, LegacyPurge::Absent | LegacyPurge::SecurelyErased)
    }
}

pub fn legacy_master_password_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join(".codeterm")
            .join("secrets")
            .join(LEGACY_MASTER_PASSWORD_FILE)
    })
}

/// Overwrite the bytes, then unlink. A failed overwrite still attempts the
/// unlink to cut exposure, but can never yield the secure discriminant.
pub fn erase_legacy_master_password_file(path: &Path) -> LegacyPurge {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LegacyPurge::Absent,
        Err(_) => return LegacyPurge::InspectionFailed,
    };
    if !meta.is_file() {
        return LegacyPurge::InspectionFailed;
    }

    let overwritten = overwrite_with_zeros(path, meta.len());
    match (std::fs::remove_file(path).is_ok(), overwritten) {
        (true, true) => LegacyPurge::SecurelyErased,
        (true, false) => LegacyPurge::RemovedWithoutOverwrite,
        (false, overwritten) => LegacyPurge::RemovalFailed { overwritten },
    }
}

/// True only when every step the erasure depends on succeeded.
fn overwrite_with_zeros(path: &Path, len: u64) -> bool {
    let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) else {
        return false;
    };
    if f.seek(SeekFrom::Start(0)).is_err() {
        return false;
    }
    let zeros = vec![0u8; len as usize];
    f.write_all(&zeros).is_ok() && f.flush().is_ok() && f.sync_all().is_ok()
}

/// Startup sweep over the well-known path.
pub fn purge_legacy_master_password_file() -> LegacyPurge {
    match legacy_master_password_path() {
        Some(p) => erase_legacy_master_password_file(&p),
        None => LegacyPurge::InspectionFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Obviously not a credential; assertions are on discriminants and residue.
    const PLACEHOLDER: &str = "BW_MASTER_PASSWORD=synthetic-not-a-credential\n";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codeterm-legacy-secret-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn set_readonly(path: &Path, readonly: bool) {
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_readonly(readonly);
        std::fs::set_permissions(path, perms).expect("set permissions");
    }

    /// Whether the erase removed the path is platform-dependent, so cleanup must
    /// never assume it survived.
    fn clear_readonly_if_present(path: &Path) {
        if path.exists() {
            set_readonly(path, false);
        }
    }

    #[test]
    fn erasing_the_legacy_file_reports_secure_and_leaves_no_residue() {
        let dir = temp_dir("erase");
        let path = dir.join(LEGACY_MASTER_PASSWORD_FILE);
        std::fs::write(&path, PLACEHOLDER).expect("seed legacy file");

        let outcome = erase_legacy_master_password_file(&path);
        assert_eq!(outcome, LegacyPurge::SecurelyErased);
        assert!(outcome.is_secure());
        assert!(!outcome.is_degraded());
        assert!(!path.exists());
        for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
            let body = std::fs::read(entry.path()).unwrap_or_default();
            assert!(!String::from_utf8_lossy(&body).contains("synthetic-not-a-credential"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_overwrite_can_never_report_secure_erasure() {
        // Read-only denies the write handle on every platform. Whether the
        // unlink then succeeds differs by platform; both are degraded outcomes.
        let dir = temp_dir("degraded");
        let path = dir.join(LEGACY_MASTER_PASSWORD_FILE);
        std::fs::write(&path, PLACEHOLDER).expect("seed legacy file");
        set_readonly(&path, true);

        let outcome = erase_legacy_master_password_file(&path);
        assert!(!outcome.is_secure(), "got {outcome:?}");
        assert!(outcome.is_degraded(), "got {outcome:?}");
        assert!(
            matches!(
                outcome,
                LegacyPurge::RemovedWithoutOverwrite
                    | LegacyPurge::RemovalFailed { overwritten: false }
            ),
            "a denied write must surface as un-overwritten, got {outcome:?}"
        );

        clear_readonly_if_present(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn erasing_an_absent_legacy_file_is_a_no_op() {
        let dir = temp_dir("absent");
        let path = dir.join(LEGACY_MASTER_PASSWORD_FILE);
        assert_eq!(
            erase_legacy_master_password_file(&path),
            LegacyPurge::Absent
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_uninspectable_path_is_not_reported_as_absent() {
        let dir = temp_dir("directory");
        let path = dir.join(LEGACY_MASTER_PASSWORD_FILE);
        std::fs::create_dir_all(&path).expect("seed a directory in the file's place");
        assert_eq!(
            erase_legacy_master_password_file(&path),
            LegacyPurge::InspectionFailed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
