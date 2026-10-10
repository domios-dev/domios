use crate::assets::{self, claim_root, AssetId, Layout};
use crate::error::{io_error, sqlite_error, StorageError};
use crate::owner::{on_owner_thread, Storage, WriterEnv};
use crate::repositories::{Completion, Revision};
use crate::schema;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupReceipt {
    pub revision: Revision,
    pub assets: u64,
    pub asset_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreReceipt {
    pub revision: Revision,
    pub assets: u64,
}

impl Storage {
    /// Writes a consistent offline image into an empty directory: SQLite's backup API copies
    /// committed state including WAL-resident commits, then every referenced asset is copied.
    /// Runs on the writer so no asset cleanup interleaves with the copy.
    pub fn backup_to(&self, destination: impl AsRef<Path>) -> Completion<BackupReceipt> {
        let destination = destination.as_ref().to_path_buf();
        self.submit_exclusive(0, move |env| backup(env, &destination))
    }

    /// Blocking and offline: verifies an image and restores it into an instance root that
    /// holds no storage. Assets land before the database, which is published last.
    pub fn restore_backup(
        image: impl AsRef<Path>,
        instance_root: impl AsRef<Path>,
    ) -> Result<RestoreReceipt, StorageError> {
        if on_owner_thread() {
            return Err(StorageError::Reentrant);
        }
        let image = Layout::in_dir(&assets::canonical_dir(image.as_ref())?);
        let claim = claim_root(instance_root.as_ref())?;
        let target = &claim.layout;
        if assets::entry_exists(&target.database)? {
            return Err(StorageError::RestoreTargetOccupied {
                root: claim.root.clone(),
            });
        }
        schema::ensure_fresh(target, &claim.root)?;
        let source = schema::open_existing(&image.database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        schema::validate_identity(&source)?;
        schema::check_integrity(&source)?;
        let revision = schema::read_meta(&source, schema::COMMIT_REVISION)?;
        let committed = assets::committed_assets(&source)?;
        let mut written = Vec::new();
        let restored = restore_into(&source, &image, target, &committed, &mut written);
        // A published database owns its blobs; only an unpublished restore rolls back.
        if restored.is_err() && !assets::entry_exists(&target.database).unwrap_or(true) {
            for path in &written {
                let _ = fs::remove_file(path);
            }
        }
        restored?;
        source
            .close()
            .map_err(|(_, e)| sqlite_error("close backup image", e))?;
        Ok(RestoreReceipt {
            revision: Revision(revision),
            assets: committed.len() as u64,
        })
    }
}

fn backup(env: &mut WriterEnv<'_>, destination: &Path) -> Result<BackupReceipt, StorageError> {
    prepare_destination(destination)?;
    let image = Layout::in_dir(&assets::canonical_dir(destination)?);
    image.create_dirs(false)?;
    assets::create_owner_file(&image.database)?;
    let mut target = schema::open_existing(&image.database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    target
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|e| sqlite_error("configure backup image", e))?;
    copy_database(env.conn, &mut target)?;
    // Reading the copied header re-opens it in WAL mode; converting then leaves a
    // self-contained rollback-journal image that opens read-only on every platform.
    schema::validate_identity(&target)?;
    let mode: String = target
        .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
        .map_err(|e| sqlite_error("detach backup image from wal", e))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(StorageError::Settings {
            setting: "journal_mode",
            expected: "delete".to_string(),
            actual: mode,
        });
    }
    schema::check_integrity(&target)?;
    let revision = schema::read_meta(&target, schema::COMMIT_REVISION)?;
    let committed = assets::committed_assets(&target)?;
    target
        .close()
        .map_err(|(_, e)| sqlite_error("close backup image", e))?;
    let mut asset_bytes = 0;
    for (id, byte_len) in &committed {
        assets::copy_blob(env.layout, &image, id, *byte_len)?;
        asset_bytes += byte_len;
    }
    assets::sync_dir(&image.blobs)?;
    assets::sync_dir(&image.dir)?;
    Ok(BackupReceipt {
        revision: Revision(revision),
        assets: committed.len() as u64,
        asset_bytes,
    })
}

fn prepare_destination(destination: &Path) -> Result<(), StorageError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error("create backup parent", e))?;
    }
    assets::ensure_owned_dir(destination)?;
    if assets::dir_has_entries(destination)? {
        return Err(StorageError::invalid(
            "backup destination",
            "must be an empty directory",
        ));
    }
    Ok(())
}

/// One step copies every page under a single read snapshot of the source.
fn copy_database(source: &Connection, target: &mut Connection) -> Result<(), StorageError> {
    let backup = Backup::new(source, target).map_err(|e| sqlite_error("start backup", e))?;
    match backup
        .step(-1)
        .map_err(|e| sqlite_error("copy database", e))?
    {
        StepResult::Done => Ok(()),
        _ => Err(StorageError::Busy {
            operation: "copy database",
        }),
    }
}

fn restore_into(
    source: &Connection,
    image: &Layout,
    target: &Layout,
    committed: &[(AssetId, u64)],
    written: &mut Vec<PathBuf>,
) -> Result<(), StorageError> {
    for (id, byte_len) in committed {
        written.push(target.blobs.join(id.as_str()));
        assets::copy_blob(image, target, id, *byte_len)?;
    }
    assets::sync_dir(&target.blobs)?;
    let staging = target
        .tmp
        .join(format!("restore-{:032x}.db", assets::unique_token()));
    written.push(staging.clone());
    assets::create_owner_file(&staging)?;
    let mut conn = schema::open_existing(&staging, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(|e| sqlite_error("configure restore staging", e))?;
    copy_database(source, &mut conn)?;
    conn.close()
        .map_err(|(_, e)| sqlite_error("close restore staging", e))?;
    assets::sync_file(&staging)?;
    assets::publish_no_replace(&staging, &target.database, &target.dir)
}
