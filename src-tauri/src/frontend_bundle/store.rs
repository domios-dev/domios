use super::{
    activation::Activation,
    manifest::{safe_component, VerifiedManifest},
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;

pub struct Store {
    root: PathBuf,
    db: Connection,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let db = Connection::open(root.join("frontend.sqlite3")).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS activation (id INTEGER PRIMARY KEY CHECK(id=1), state TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS checkpoints (scope TEXT PRIMARY KEY, revision INTEGER NOT NULL, payload TEXT NOT NULL);")
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root: root.into(),
            db,
        })
    }

    pub fn load(&self) -> Result<Activation, String> {
        let value: Option<String> = self
            .db
            .query_row("SELECT state FROM activation WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        value
            .map(|value| serde_json::from_str(&value).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(Activation::default()))
    }

    pub fn save(&mut self, state: &Activation) -> Result<(), String> {
        let payload = serde_json::to_string(state).map_err(|e| e.to_string())?;
        let transaction = self.db.transaction().map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO activation(id,state) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET state=excluded.state", [payload])
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())
    }

    pub(super) fn install(
        &self,
        verified: &VerifiedManifest,
        signature: &str,
        archive: &[u8],
    ) -> Result<(), String> {
        let manifest = verified.manifest();
        use sha2::{Digest, Sha256};
        if format!("{:x}", Sha256::digest(archive)) != manifest.sha256 {
            return Err("Verified archive bytes changed".into());
        }
        let destination = self.root.join("releases").join(&manifest.release_id);
        if destination.exists() {
            let stored = fs::read(destination.join("manifest.json")).map_err(|e| e.to_string())?;
            if stored != manifest.canonical_bytes()? {
                return Err("Immutable release identity collision".into());
            }
            let expected = tempfile::tempdir().map_err(|e| e.to_string())?;
            extract(archive, expected.path())?;
            compare_assets(
                &expected.path().join("assets-root"),
                &destination.join("assets-root"),
            )?;
            return Ok(());
        }
        let releases = self.root.join("releases");
        fs::create_dir_all(&releases).map_err(|e| e.to_string())?;
        let staging = tempfile::tempdir_in(&releases).map_err(|e| e.to_string())?;
        extract(archive, staging.path())?;
        write_synced(
            &staging.path().join("manifest.json"),
            &manifest.canonical_bytes()?,
        )?;
        write_synced(
            &staging.path().join("manifest.minisig"),
            signature.as_bytes(),
        )?;
        write_synced(&staging.path().join("bundle.tar.gz"), archive)?;
        fs::rename(staging.path(), &destination).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::File::open(&releases)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn verify_release(
        &self,
        manifest: &super::manifest::Manifest,
        key: &str,
        api: u32,
        allow_loopback: bool,
    ) -> Result<VerifiedManifest, String> {
        if !safe_component(&manifest.release_id) {
            return Err("Invalid release ID".into());
        }
        let directory = self.root.join("releases").join(&manifest.release_id);
        let metadata = fs::read(directory.join("manifest.json")).map_err(|e| e.to_string())?;
        let signature =
            fs::read_to_string(directory.join("manifest.minisig")).map_err(|e| e.to_string())?;
        let archive = fs::read(directory.join("bundle.tar.gz")).map_err(|e| e.to_string())?;
        let verified =
            super::manifest::verify(&metadata, &signature, &archive, key, api, allow_loopback)?;
        if verified.manifest() != manifest {
            return Err("Stored metadata mismatch".into());
        }
        let expected = tempfile::tempdir().map_err(|e| e.to_string())?;
        extract(&archive, expected.path())?;
        compare_assets(
            &expected.path().join("assets-root"),
            &directory.join("assets-root"),
        )?;
        Ok(verified)
    }

    pub fn asset(&self, release: &str, path: &str) -> Result<Vec<u8>, String> {
        if !safe_component(release) || !safe_asset_path(path) {
            return Err("Invalid asset path".into());
        }
        let base = self.root.join("releases").join(release).join("assets-root");
        let target = base.join(path);
        let metadata = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Invalid asset type".into());
        }
        let canonical = target.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(base.canonicalize().map_err(|e| e.to_string())?) {
            return Err("Asset outside release".into());
        }
        fs::read(canonical).map_err(|e| e.to_string())
    }

    pub fn checkpoint(&self, scope: &str) -> Result<Option<(u64, serde_json::Value)>, String> {
        let value: Option<(u64, String)> = self
            .db
            .query_row(
                "SELECT revision,payload FROM checkpoints WHERE scope=?1",
                [scope],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        value
            .map(|(revision, payload)| {
                serde_json::from_str(&payload)
                    .map(|payload| (revision, payload))
                    .map_err(|e| e.to_string())
            })
            .transpose()
    }

    pub fn put_checkpoint(
        &mut self,
        scope: &str,
        revision: u64,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        if revision > i64::MAX as u64 {
            return Err("Revision exceeds storage range".into());
        }
        let encoded = serde_json::to_string(payload).map_err(|e| e.to_string())?;
        if encoded.len() > 1024 * 1024 {
            return Err("Checkpoint exceeds limit".into());
        }
        let transaction = self.db.transaction().map_err(|e| e.to_string())?;
        let existing: Option<(u64, String)> = transaction
            .query_row(
                "SELECT revision,payload FROM checkpoints WHERE scope=?1",
                [scope],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if existing
            .as_ref()
            .is_some_and(|(old, data)| *old > revision || (*old == revision && data != &encoded))
        {
            return Err("Checkpoint revision conflict".into());
        }
        transaction.execute("INSERT INTO checkpoints(scope,revision,payload) VALUES(?1,?2,?3) ON CONFLICT(scope) DO UPDATE SET revision=excluded.revision,payload=excluded.payload", params![scope, revision, encoded]).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())
    }
}

pub fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !is_device_name(part))
}

fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || part.ends_with(['.', ' '])
}

pub fn extract(archive: &[u8], directory: &Path) -> Result<(), String> {
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err("Archive exceeds limit".into());
    }
    let mut unpacked = Vec::new();
    flate2::read::GzDecoder::new(archive)
        .take(MAX_EXPANDED_BYTES + 1)
        .read_to_end(&mut unpacked)
        .map_err(|e| e.to_string())?;
    if unpacked.len() as u64 > MAX_EXPANDED_BYTES
        || unpacked.len() < 1024
        || unpacked.len() % 512 != 0
        || unpacked[unpacked.len() - 1024..].iter().any(|b| *b != 0)
    {
        return Err("Invalid or truncated tar archive".into());
    }
    let mut names = BTreeSet::new();
    let mut tar = tar::Archive::new(unpacked.as_slice());
    let root = directory.join("assets-root");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let name = path.to_str().ok_or("Non-UTF8 asset path")?.to_string();
        if !safe_asset_path(&name)
            || !entry.header().entry_type().is_file()
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err("Unsafe or duplicate archive entry".into());
        }
        let destination = root.join(&name);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    if !names.contains("index.html") {
        return Err("Missing root index".into());
    }
    Ok(())
}

fn write_synced(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|e| e.to_string())?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn compare_assets(expected: &Path, actual: &Path) -> Result<(), String> {
    for entry in fs::read_dir(expected).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = actual.join(entry.file_name());
        let kind = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if kind.file_type().is_symlink() {
            return Err("Stored asset symlink".into());
        }
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            if !kind.is_dir() {
                return Err("Stored asset directory mismatch".into());
            }
            compare_assets(&entry.path(), &target)?;
        } else if !kind.is_file()
            || fs::read(entry.path()).map_err(|e| e.to_string())?
                != fs::read(target).map_err(|e| e.to_string())?
        {
            return Err("Stored asset digest mismatch".into());
        }
    }
    if fs::read_dir(expected).map_err(|e| e.to_string())?.count()
        != fs::read_dir(actual).map_err(|e| e.to_string())?.count()
    {
        return Err("Unexpected stored asset".into());
    }
    Ok(())
}
