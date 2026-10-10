use crate::error::{io_error, sqlite_error, StorageError};
use crate::owner::{lock, FaultPoint, Faults, Storage, WriteCx};
use crate::repositories::{from_sql_u64, to_sql_u64, Completion};
use crate::schema::ASSET_REFERENCE_TABLES;
use rusqlite::{params, Connection};
use std::collections::hash_map::RandomState;
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::hash::BuildHasher;
use std::io::{ErrorKind, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const STORAGE_DIR: &str = "storage";
const MAX_SCAN_ENTRIES: usize = 4096;
const MAX_CLEANUP_BATCH: usize = 1024;

/// Storage-owned paths; everything under `<instance root>/storage` belongs to Storage.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub(crate) dir: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) lock: PathBuf,
    pub(crate) blobs: PathBuf,
    pub(crate) tmp: PathBuf,
}

impl Layout {
    pub(crate) fn of_root(root: &Path) -> Layout {
        Layout::in_dir(&root.join(STORAGE_DIR))
    }

    pub(crate) fn in_dir(dir: &Path) -> Layout {
        Layout {
            dir: dir.to_path_buf(),
            database: dir.join("state.db"),
            lock: dir.join("storage.lock"),
            blobs: dir.join("assets").join("blobs"),
            tmp: dir.join("tmp"),
        }
    }

    pub(crate) fn database_sidecar(&self, suffix: &str) -> PathBuf {
        let mut text = self.database.as_os_str().to_os_string();
        text.push(suffix);
        text.into()
    }

    pub(crate) fn create_dirs(&self, with_tmp: bool) -> Result<(), StorageError> {
        ensure_owned_dir(&self.dir)?;
        ensure_owned_dir(&self.dir.join("assets"))?;
        ensure_owned_dir(&self.blobs)?;
        if with_tmp {
            ensure_owned_dir(&self.tmp)?;
        }
        Ok(())
    }
}

struct RegistryKey(String);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        lock(registry()).remove(&self.0);
    }
}

fn registry() -> &'static Mutex<HashSet<String>> {
    static ROOTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ROOTS.get_or_init(Default::default)
}

/// Exclusive ownership of one instance root: an in-process canonical key plus an OS
/// lock, which also catches aliases the canonical form cannot unify.
pub(crate) struct RootClaim {
    _lock: File,
    _key: RegistryKey,
    pub(crate) root: PathBuf,
    pub(crate) layout: Layout,
}

impl Drop for RootClaim {
    fn drop(&mut self) {
        let _ = self._lock.unlock();
    }
}

/// Absolute, link-resolved spelling without the Windows verbatim prefix; that prefix is
/// added back only by `native_path` for the Win32 and SQLite calls Storage makes itself.
pub(crate) fn canonical_dir(path: &Path) -> Result<PathBuf, StorageError> {
    let canonical = fs::canonicalize(path).map_err(|e| io_error("resolve storage path", e))?;
    let text = canonical
        .to_str()
        .ok_or_else(|| StorageError::RootInvalid {
            root: path.to_path_buf(),
            reason: "path is not valid UTF-8".to_string(),
        })?;
    Ok(PathBuf::from(codeterm_paths::strip_verbatim_prefix(text)))
}

/// Extended-length (`\\?\`) form of an absolute normalized path for direct Win32 and
/// SQLite calls; std adds its own prefix. Anything else is returned unchanged.
#[cfg(windows)]
pub(crate) fn native_path(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let mut native = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) => OsString::from(format!(r"\\?\{}:", drive as char)),
            Prefix::UNC(server, share) => {
                let mut unc = OsString::from(r"\\?\UNC\");
                unc.push(server);
                unc.push(r"\");
                unc.push(share);
                unc
            }
            _ => return path.to_path_buf(),
        },
        _ => return path.to_path_buf(),
    };
    if components.next() != Some(Component::RootDir) {
        return path.to_path_buf();
    }
    for component in components {
        let Component::Normal(name) = component else {
            return path.to_path_buf();
        };
        native.push(r"\");
        native.push(name);
    }
    native.into()
}

pub(crate) fn claim_root(requested: &Path) -> Result<RootClaim, StorageError> {
    fs::create_dir_all(requested).map_err(|e| io_error("create instance root", e))?;
    let root = canonical_dir(requested)?;
    let spelling = root.to_string_lossy();
    let key = if cfg!(windows) {
        spelling.to_lowercase()
    } else {
        spelling.into_owned()
    };
    if !lock(registry()).insert(key.clone()) {
        return Err(StorageError::AlreadyOpen { root });
    }
    let key = RegistryKey(key);
    let layout = Layout::of_root(&root);
    layout.create_dirs(true)?;
    let lock_file = open_lock_file(&layout.lock)?;
    match lock_file.try_lock() {
        Ok(()) => Ok(RootClaim {
            _lock: lock_file,
            _key: key,
            root,
            layout,
        }),
        Err(TryLockError::WouldBlock) => Err(StorageError::Locked { root }),
        Err(TryLockError::Error(error)) => Err(io_error("lock storage root", error)),
    }
}

/// Storage-generated asset identity: 32 lowercase hex digits, never a caller path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(String);

impl AssetId {
    pub fn parse(text: &str) -> Result<AssetId, StorageError> {
        let valid =
            text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !valid {
            return Err(StorageError::invalid(
                "asset id",
                "must be 32 lowercase hex digits",
            ));
        }
        Ok(AssetId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generate() -> AssetId {
        AssetId(format!("{:032x}", unique_token()))
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A committed immutable asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef {
    pub id: AssetId,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCleanup {
    pub asset: AssetId,
    pub last_error: Option<String>,
}

/// Writer-ordered view of the file + metadata protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetAudit {
    pub committed: u64,
    /// Committed metadata whose file is missing or has the wrong size; must stay empty.
    pub dangling: Vec<AssetId>,
    /// Published files without committed metadata, left by a rolled-back operation.
    pub orphan_blobs: Vec<AssetId>,
    pub pending_cleanup: Vec<PendingCleanup>,
    pub temp_files: u64,
    pub unknown_entries: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub removed_blobs: u64,
    pub removed_temp: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed: u64,
    pub retained: Vec<PendingCleanup>,
    /// Still reachable from an open read snapshot; removed once that snapshot ends.
    pub deferred: u64,
}

/// Asset maintenance; references themselves belong to typed domain repositories.
#[derive(Clone)]
pub struct AssetRepository {
    storage: Storage,
}

impl AssetRepository {
    pub(crate) fn new(storage: Storage) -> Self {
        AssetRepository { storage }
    }

    pub fn audit(&self) -> Completion<AssetAudit> {
        self.storage
            .submit_exclusive(0, |env| audit(env.conn, env.layout))
    }

    /// Reclaims orphaned published files and staging files; committed assets are untouched.
    pub fn sweep_orphans(&self) -> Completion<SweepReport> {
        self.storage
            .submit_exclusive(0, |env| sweep(env.conn, env.layout))
    }

    /// Retries the bounded set of released files; files an open read snapshot can still
    /// reach stay queued and are reported as deferred.
    pub fn run_cleanup(&self) -> Completion<CleanupReport> {
        self.storage.submit_exclusive(0, |env| {
            env.releases.defer(pending_releases(env.conn)?);
            env.releases.drain(env.conn, env.layout, env.faults)
        })
    }
}

pub(crate) fn unique_token() -> u128 {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mix = |salt: u64| RandomState::new().hash_one((salt, sequence, nanos, std::process::id()));
    (u128::from(mix(1)) << 64) | u128::from(mix(2))
}

pub(crate) fn entry_exists(path: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error("inspect storage path", error)),
    }
}

pub(crate) fn dir_has_entries(dir: &Path) -> Result<bool, StorageError> {
    let mut entries = fs::read_dir(dir).map_err(|e| io_error("list storage directory", e))?;
    Ok(entries.next().is_some())
}

/// Links and every Windows reparse point (symlinks, junctions, mount points) are refused.
fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn is_owned_file(meta: &fs::Metadata) -> bool {
    meta.is_file() && !is_link_or_reparse(meta)
}

fn not_confined(path: &Path, reason: &str) -> StorageError {
    StorageError::RootInvalid {
        root: path.to_path_buf(),
        reason: format!("storage path {reason}"),
    }
}

/// Storage directories are real directories, never links out of the owned root.
pub(crate) fn ensure_owned_dir(path: &Path) -> Result<(), StorageError> {
    let real_dir = |meta: &fs::Metadata| meta.is_dir() && !is_link_or_reparse(meta);
    match fs::symlink_metadata(path) {
        Ok(meta) if real_dir(&meta) => Ok(()),
        Ok(_) => Err(not_confined(path, "is not a real directory")),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            #[cfg(unix)]
            let builder = {
                let mut builder = fs::DirBuilder::new();
                std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
                builder
            };
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            match builder.create(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    match fs::symlink_metadata(path) {
                        Ok(meta) if real_dir(&meta) => Ok(()),
                        _ => Err(not_confined(path, "changed while being created")),
                    }
                }
                Err(error) => Err(io_error("create storage directory", error)),
            }
        }
        Err(error) => Err(io_error("inspect storage directory", error)),
    }
}

/// New owner-only file; never replaces an existing entry.
pub(crate) fn create_owner_file(path: &Path) -> Result<File, StorageError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .map_err(|e| io_error("create storage file", e))
}

pub(crate) fn open_lock_file(path: &Path) -> Result<File, StorageError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| !is_owned_file(&meta)) {
        return Err(not_confined(path, "lock is not a regular file"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .map_err(|e| io_error("open storage lock", e))
}

/// Windows FlushFileBuffers needs write access, so the handle is writable everywhere.
pub(crate) fn sync_file(path: &Path) -> Result<(), StorageError> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| io_error("sync storage file", e))
}

/// Persists a directory entry change on Unix. Windows publication flushes through
/// MoveFileExW instead; std cannot open a Windows directory handle for flushing.
pub(crate) fn sync_dir(dir: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        File::open(dir)
            .and_then(|handle| handle.sync_all())
            .map_err(|e| io_error("sync storage directory", e))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Publishes a fully synced staged file under `target`, atomically refusing to replace any
/// existing entry. Unix: link(2) fails with EEXIST, then the directory is synced and the
/// staging name dropped. Windows: MoveFileExW without REPLACE_EXISTING, with WRITE_THROUGH.
pub(crate) fn publish_no_replace(
    staged: &Path,
    target: &Path,
    dir: &Path,
) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        fs::hard_link(staged, target).map_err(|e| io_error("publish storage file", e))?;
        sync_dir(dir)?;
        // A surviving staging name is a reclaimable duplicate; publication already happened.
        let _ = fs::remove_file(staged);
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = dir;
        move_no_replace(staged, target).map_err(|e| io_error("publish storage file", e))
    }
}

#[cfg(windows)]
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let wide =
        |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (from, to) = (wide(&native_path(from)), wide(&native_path(to)));
    // SAFETY: both buffers are NUL-terminated UTF-16 paths that outlive the call.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let mut file = create_owner_file(path)?;
    file.write_all(bytes)
        .map_err(|e| io_error("write asset", e))?;
    file.sync_all().map_err(|e| io_error("sync asset", e))
}

/// Publishes a synced immutable file, then inserts its metadata in the caller's savepoint.
/// A later rollback leaves a reclaimable orphan, never a committed dangling reference.
pub(crate) fn publish(cx: &mut WriteCx<'_>, bytes: &[u8]) -> Result<AssetRef, StorageError> {
    if bytes.len() > cx.max_asset_bytes {
        return Err(StorageError::TooLarge {
            bytes: bytes.len(),
            limit: cx.max_asset_bytes,
        });
    }
    let id = cx.faults.take_asset_id().unwrap_or_else(AssetId::generate);
    let staged = cx.layout.tmp.join(format!("asset-{id}.part"));
    write_new_file(&staged, bytes)?;
    if cx.faults.take(FaultPoint::AssetPublish) {
        return Err(StorageError::Injected {
            point: "asset publish",
        });
    }
    let target = cx.layout.blobs.join(id.as_str());
    if let Err(error) = publish_no_replace(&staged, &target, &cx.layout.blobs) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    let byte_len = bytes.len() as u64;
    let revision = to_sql_u64("revision", cx.revision.0)?;
    cx.write(
        "INSERT INTO assets (id, byte_len, created_revision) VALUES (?1, ?2, ?3)",
        params![id.as_str(), to_sql_u64("asset size", byte_len)?, revision],
    )?;
    Ok(AssetRef { id, byte_len })
}

/// Drops the metadata of an asset no table references and queues its file for post-commit removal.
pub(crate) fn release_if_unreferenced(
    cx: &mut WriteCx<'_>,
    id: &AssetId,
) -> Result<bool, StorageError> {
    for table in ASSET_REFERENCE_TABLES {
        let references: i64 = cx
            .conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE asset_id = ?1"),
                [id.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| sqlite_error("count asset references", e))?;
        if references > 0 {
            return Ok(false);
        }
    }
    cx.write("DELETE FROM assets WHERE id = ?1", [id.as_str()])?;
    let revision = to_sql_u64("revision", cx.revision.0)?;
    cx.conn
        .execute(
            "INSERT INTO asset_cleanup (asset_id, released_revision, last_error) \
             VALUES (?1, ?2, NULL)",
            params![id.as_str(), revision],
        )
        .map_err(|e| sqlite_error("queue asset cleanup", e))?;
    cx.released.push(id.clone());
    Ok(true)
}

/// Removes released files after their reference removal committed; a failure stays
/// recorded in `asset_cleanup` for an explicit retry.
pub(crate) fn remove_released(
    conn: &Connection,
    layout: &Layout,
    faults: &mut Faults,
    ids: &[AssetId],
) -> Result<CleanupReport, StorageError> {
    let mut report = CleanupReport::default();
    let mut outcomes = Vec::with_capacity(ids.len());
    for id in ids {
        outcomes.push((id, remove_blob(layout, id, faults)));
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| sqlite_error("begin cleanup", e))?;
    for (id, outcome) in outcomes {
        let recorded = match &outcome {
            Ok(()) => tx.execute(
                "DELETE FROM asset_cleanup WHERE asset_id = ?1",
                [id.as_str()],
            ),
            Err(error) => tx.execute(
                "UPDATE asset_cleanup SET last_error = ?2 WHERE asset_id = ?1",
                params![id.as_str(), error.to_string()],
            ),
        };
        recorded.map_err(|e| sqlite_error("record cleanup", e))?;
        match outcome {
            Ok(()) => report.removed += 1,
            Err(error) => report.retained.push(PendingCleanup {
                asset: id.clone(),
                last_error: Some(error.to_string()),
            }),
        }
    }
    tx.commit().map_err(|e| sqlite_error("commit cleanup", e))?;
    Ok(report)
}

fn remove_blob(layout: &Layout, id: &AssetId, faults: &mut Faults) -> Result<(), StorageError> {
    if faults.take(FaultPoint::Cleanup) {
        return Err(StorageError::Injected {
            point: "asset cleanup",
        });
    }
    match fs::remove_file(layout.blobs.join(id.as_str())) {
        Ok(()) => sync_dir(&layout.blobs),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove asset", error)),
    }
}

fn corrupt(id: &AssetId, detail: impl Into<String>) -> StorageError {
    StorageError::AssetCorrupt {
        asset: id.to_string(),
        detail: detail.into(),
    }
}

fn open_committed(layout: &Layout, id: &AssetId, byte_len: u64) -> Result<File, StorageError> {
    let path = layout.blobs.join(id.as_str());
    let meta = fs::symlink_metadata(&path).map_err(|e| corrupt(id, e.to_string()))?;
    if !is_owned_file(&meta) {
        return Err(corrupt(id, "not a regular owned file"));
    }
    let file = File::open(&path).map_err(|e| corrupt(id, e.to_string()))?;
    let actual = file
        .metadata()
        .map_err(|e| io_error("inspect asset", e))?
        .len();
    if actual != byte_len {
        return Err(corrupt(
            id,
            format!("{actual} bytes on disk, {byte_len} committed"),
        ));
    }
    Ok(file)
}

pub(crate) fn read_blob(
    layout: &Layout,
    id: &AssetId,
    byte_len: u64,
) -> Result<Vec<u8>, StorageError> {
    let file = open_committed(layout, id, byte_len)?;
    let mut bytes = Vec::with_capacity(usize::try_from(byte_len).unwrap_or_default());
    file.take(byte_len)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read asset", e))?;
    if bytes.len() as u64 != byte_len {
        return Err(corrupt(id, "short read"));
    }
    Ok(bytes)
}

/// Copies one committed asset into another Storage layout and syncs it.
pub(crate) fn copy_blob(
    from: &Layout,
    to: &Layout,
    id: &AssetId,
    byte_len: u64,
) -> Result<(), StorageError> {
    let source = open_committed(from, id, byte_len)?;
    let mut target = create_owner_file(&to.blobs.join(id.as_str()))?;
    let copied = std::io::copy(&mut source.take(byte_len), &mut target)
        .map_err(|e| io_error("copy asset", e))?;
    if copied != byte_len {
        return Err(corrupt(id, "short copy"));
    }
    target.sync_all().map_err(|e| io_error("sync asset", e))
}

pub(crate) fn committed_assets(conn: &Connection) -> Result<Vec<(AssetId, u64)>, StorageError> {
    let mut statement = conn
        .prepare("SELECT id, byte_len FROM assets ORDER BY id")
        .map_err(|e| sqlite_error("list assets", e))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| sqlite_error("list assets", e))?;
    rows.map(|row| {
        let (id, byte_len) = row.map_err(|e| sqlite_error("list assets", e))?;
        let id = AssetId::parse(&id).map_err(|_| StorageError::Corrupt {
            detail: "malformed asset id".to_string(),
        })?;
        Ok((id, from_sql_u64("asset size", byte_len)?))
    })
    .collect()
}

fn pending_cleanup(conn: &Connection) -> Result<Vec<PendingCleanup>, StorageError> {
    let mut statement = conn
        .prepare(
            "SELECT asset_id, last_error FROM asset_cleanup \
             ORDER BY released_revision, asset_id LIMIT ?1",
        )
        .map_err(|e| sqlite_error("list asset cleanup", e))?;
    let rows = statement
        .query_map([MAX_CLEANUP_BATCH as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| sqlite_error("list asset cleanup", e))?;
    rows.map(|row| {
        let (asset, last_error) = row.map_err(|e| sqlite_error("list asset cleanup", e))?;
        let asset = AssetId::parse(&asset).map_err(|_| StorageError::Corrupt {
            detail: "malformed asset id".to_string(),
        })?;
        Ok(PendingCleanup { asset, last_error })
    })
    .collect()
}

/// Released assets still awaiting file removal, with the revision that released each.
pub(crate) fn pending_releases(conn: &Connection) -> Result<Vec<(AssetId, u64)>, StorageError> {
    let mut statement = conn
        .prepare(
            "SELECT asset_id, released_revision FROM asset_cleanup \
             ORDER BY released_revision, asset_id LIMIT ?1",
        )
        .map_err(|e| sqlite_error("list asset cleanup", e))?;
    let rows = statement
        .query_map([MAX_CLEANUP_BATCH as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| sqlite_error("list asset cleanup", e))?;
    rows.map(|row| {
        let (asset, released) = row.map_err(|e| sqlite_error("list asset cleanup", e))?;
        let asset = AssetId::parse(&asset).map_err(|_| StorageError::Corrupt {
            detail: "malformed asset id".to_string(),
        })?;
        Ok((asset, from_sql_u64("released revision", released)?))
    })
    .collect()
}

fn scan(dir: &Path) -> Result<Vec<String>, StorageError> {
    let entries = fs::read_dir(dir).map_err(|e| io_error("list storage directory", e))?;
    entries
        .take(MAX_SCAN_ENTRIES)
        .map(|entry| {
            let entry = entry.map_err(|e| io_error("list storage directory", e))?;
            Ok(entry.file_name().to_string_lossy().into_owned())
        })
        .collect()
}

fn audit(conn: &Connection, layout: &Layout) -> Result<AssetAudit, StorageError> {
    let committed = committed_assets(conn)?;
    let pending = pending_cleanup(conn)?;
    let mut report = AssetAudit {
        committed: committed.len() as u64,
        ..AssetAudit::default()
    };
    for (id, byte_len) in &committed {
        let on_disk = fs::symlink_metadata(layout.blobs.join(id.as_str()))
            .ok()
            .filter(is_owned_file);
        if on_disk.map(|meta| meta.len()) != Some(*byte_len) {
            report.dangling.push(id.clone());
        }
    }
    let known: HashSet<&str> = committed
        .iter()
        .map(|(id, _)| id.as_str())
        .chain(pending.iter().map(|entry| entry.asset.as_str()))
        .collect();
    for name in scan(&layout.blobs)? {
        match AssetId::parse(&name) {
            Ok(id) if !known.contains(id.as_str()) => report.orphan_blobs.push(id),
            Ok(_) => {}
            Err(_) => report.unknown_entries += 1,
        }
    }
    report.temp_files = scan(&layout.tmp)?.len() as u64;
    report.pending_cleanup = pending;
    Ok(report)
}

fn sweep(conn: &Connection, layout: &Layout) -> Result<SweepReport, StorageError> {
    let orphans = audit(conn, layout)?.orphan_blobs;
    let mut report = SweepReport::default();
    for id in &orphans {
        fs::remove_file(layout.blobs.join(id.as_str()))
            .map_err(|e| io_error("remove orphan asset", e))?;
        report.removed_blobs += 1;
    }
    if report.removed_blobs > 0 {
        sync_dir(&layout.blobs)?;
    }
    for name in scan(&layout.tmp)? {
        let path = layout.tmp.join(&name);
        if fs::symlink_metadata(&path).is_ok_and(|meta| is_owned_file(&meta)) {
            fs::remove_file(&path).map_err(|e| io_error("remove staging file", e))?;
            report.removed_temp += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod root_claim_tests {
    use super::*;

    #[test]
    fn storage_root_claim_releases_lock_with_retained_descriptor() {
        let root = std::env::temp_dir().join(format!(
            "codeterm-root-claim-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let claim = claim_root(&root).unwrap();
        let inherited = claim._lock.try_clone().unwrap();
        assert!(matches!(
            claim_root(&root),
            Err(StorageError::AlreadyOpen { .. })
        ));
        let probe = open_lock_file(&claim.layout.lock).unwrap();
        assert!(matches!(probe.try_lock(), Err(TryLockError::WouldBlock)));
        drop(claim);
        let reopened = claim_root(&root).unwrap();
        assert!(matches!(probe.try_lock(), Err(TryLockError::WouldBlock)));
        drop(reopened);
        probe.try_lock().unwrap();
        probe.unlock().unwrap();
        drop(inherited);
        drop(probe);
        fs::remove_dir_all(root).unwrap();
    }
}
