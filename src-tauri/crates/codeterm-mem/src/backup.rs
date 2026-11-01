use codeterm_paths::{instance_identity::ResolvedInstancePaths, path_owner::PathOwnership};
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, String>,
}

fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn private_directory(path: &Path) -> io::Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path)
}

fn plain(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
        return Err(failure(
            "Backup paths must be ordinary files and directories",
        ));
    }
    Ok(metadata)
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    if !plain(source)?.is_dir() {
        return Err(failure("Backup source is not a directory"));
    }
    private_directory(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let output = target.join(entry.file_name());
        if plain(&path)?.is_dir() {
            copy_tree(&path, &output)?;
        } else {
            fs::copy(path, output)?;
        }
    }
    Ok(())
}

fn digest(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn inventory(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> io::Result<()> {
    if !plain(path)?.is_dir() {
        return Err(failure("Backup inventory is not a directory"));
    }
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if plain(&path)?.is_dir() {
            inventory(root, &path, files)?;
        } else {
            let relative = path.strip_prefix(root).map_err(failure)?;
            let key = relative
                .to_str()
                .ok_or_else(|| failure("Non-Unicode backup path"))?
                .replace('\\', "/");
            files.insert(key, digest(&path)?);
        }
    }
    Ok(())
}

fn manifest(root: &Path) -> io::Result<Manifest> {
    let mut files = BTreeMap::new();
    for directory in ["mem", "storage"] {
        inventory(root, &root.join(directory), &mut files)?;
    }
    Ok(Manifest { version: 1, files })
}

pub fn create(data_dir: &Path, out: &Path) -> io::Result<PathBuf> {
    let data_dir = data_dir.canonicalize()?;
    fs::create_dir_all(out)?;
    let out = out.canonicalize()?;
    if out.starts_with(&data_dir) {
        return Err(failure("Backup output must be outside the data directory"));
    }
    let name = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        uuid::Uuid::new_v4()
    );
    let temporary = out.join(format!(".incomplete-{name}"));
    private_directory(&temporary)?;
    private_directory(&temporary.join("mem"))?;
    let result = (|| {
        snapshot(
            &data_dir.join("mem/knowledge.db"),
            &temporary.join("mem/knowledge.db"),
        )?;
        for directory in ["mem/blobs", "storage"] {
            let source = data_dir.join(directory);
            match fs::symlink_metadata(&source) {
                Ok(_) => copy_tree(&source, &temporary.join(directory))?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    private_directory(&temporary.join(directory))?
                }
                Err(error) => return Err(error),
            }
        }
        let manifest = manifest(&temporary)?;
        fs::write(
            temporary.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(failure)?,
        )?;
        let complete = out.join(name);
        fs::rename(&temporary, &complete)?;
        Ok(complete)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn snapshot(source: &Path, target: &Path) -> io::Result<()> {
    snapshot_with_progress(source, target, || {})
}

fn snapshot_with_progress(
    source: &Path,
    target: &Path,
    mut progress: impl FnMut(),
) -> io::Result<()> {
    if !plain(source)?.is_file() {
        return Err(failure("knowledge.db is not a file"));
    }
    let source =
        Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(failure)?;
    let mut target = Connection::open(target).map_err(failure)?;
    {
        let backup = Backup::new(&source, &mut target).map_err(failure)?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if Instant::now() >= deadline {
                return Err(failure("Online SQLite backup deadline exceeded"));
            }
            match backup.step(128).map_err(failure)? {
                StepResult::Done => break,
                StepResult::More => progress(),
                _ => {
                    return Err(failure(
                        "Online SQLite backup is busy; no complete backup was published",
                    ))
                }
            }
        }
    }
    target
        .pragma_update(None, "journal_mode", "DELETE")
        .map_err(failure)?;
    target.close().map_err(|(_, error)| failure(error))
}

fn validate(source: &Path) -> io::Result<()> {
    plain(source)?;
    plain(&source.join("manifest.json"))?;
    let expected: Manifest =
        serde_json::from_slice(&fs::read(source.join("manifest.json"))?).map_err(failure)?;
    if expected.version != 1 || !expected.files.contains_key("mem/knowledge.db") {
        return Err(failure("Unsupported or incomplete backup manifest"));
    }
    for key in expected.files.keys() {
        let path = Path::new(key);
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
            || !(key == "mem/knowledge.db"
                || key.starts_with("mem/blobs/")
                || key.starts_with("storage/"))
        {
            return Err(failure("Invalid backup manifest path"));
        }
    }
    if manifest(source)? != expected {
        return Err(failure("Backup integrity mismatch"));
    }
    let db = Connection::open_with_flags(
        source.join("mem/knowledge.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(failure)?;
    let check: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(failure)?;
    if check != "ok" {
        return Err(failure("SQLite backup integrity check failed"));
    }
    Ok(())
}

pub fn restore(
    source: &Path,
    identity: ResolvedInstancePaths,
    registry: &Path,
) -> io::Result<PathBuf> {
    let _owner = PathOwnership::try_acquire(identity.clone(), registry).map_err(|error| {
        failure(format!(
            "Restore requires a stopped daemon and exclusive data ownership: {error}"
        ))
    })?;
    let source = source.canonicalize()?;
    let data = identity.data();
    if source.starts_with(data) || data.starts_with(&source) {
        return Err(failure("Restore source must be outside the data directory"));
    }
    validate(&source)?;
    fs::create_dir_all(data)?;
    let staging = data.join(format!(".restore-{}", uuid::Uuid::new_v4()));
    private_directory(&staging)?;
    for directory in ["mem", "storage"] {
        copy_tree(&source.join(directory), &staging.join(directory))?;
    }
    fs::copy(source.join("manifest.json"), staging.join("manifest.json"))?;
    validate(&staging)?;
    let previous = data.join(format!(".before-restore-{}", uuid::Uuid::new_v4()));
    private_directory(&previous)?;
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let result = (|| {
        for directory in ["mem", "storage"] {
            let target = data.join(directory);
            if target.try_exists()? {
                plain(&target)?;
                fs::rename(&target, previous.join(directory))?;
                moved.push(directory);
            }
            fs::rename(staging.join(directory), &target)?;
            installed.push(directory);
        }
        Ok(())
    })();
    if let Err(error) = result {
        for directory in installed.into_iter().rev() {
            fs::rename(data.join(directory), staging.join(directory))?;
        }
        for directory in moved.into_iter().rev() {
            fs::rename(previous.join(directory), data.join(directory))?;
        }
        return Err(error);
    }
    fs::remove_dir_all(staging)?;
    Ok(previous)
}

#[cfg(test)]
mod tests;
