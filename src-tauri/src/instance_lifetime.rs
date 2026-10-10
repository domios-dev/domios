use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

const REGISTRATION: &str = "codeterm-instance-lifetime-v1";

pub(crate) struct InstanceLifetime {
    _lock: File,
}

pub(crate) struct GoneInstance {
    pub(crate) instance_id: String,
    _lock: File,
}

fn registry_dir() -> io::Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".codeterm-instance-lifetimes"))
        .ok_or_else(|| io::Error::other("Could not determine home directory"))
}

fn canonical_instance_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}

fn registration(id: &str) -> String {
    format!("{REGISTRATION}\n{id}\n")
}

fn registered_instance(name: &str, contents: &str) -> Option<String> {
    let id = name.strip_suffix(".lock")?;
    (canonical_instance_id(id) && contents == registration(id)).then(|| id.to_string())
}

fn regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

pub(crate) fn acquire() -> io::Result<InstanceLifetime> {
    acquire_at(&registry_dir()?, crate::paths::codeterm_instance_id())
}

fn acquire_at(root: &Path, id: &str) -> io::Result<InstanceLifetime> {
    if !canonical_instance_id(id) {
        return Err(io::Error::other("Invalid instance identity"));
    }
    std::fs::create_dir_all(root)?;
    if !real_directory(root) {
        return Err(io::Error::other(
            "Instance registry is not a regular directory",
        ));
    }
    let path = root.join(format!("{id}.lock"));
    if path.try_exists()? && !regular_file(&path) {
        return Err(io::Error::other(
            "Instance registration is not a regular file",
        ));
    }
    // Never replace or unlink a lifetime lock: all contenders must lock the same inode.
    let mut lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    lock.try_lock().map_err(io::Error::other)?;
    lock.set_len(0)?;
    lock.write_all(registration(id).as_bytes())?;
    lock.sync_all()?;
    Ok(InstanceLifetime { _lock: lock })
}

pub(crate) fn lock_gone_instances() -> Vec<GoneInstance> {
    if !codeterm_plugin::global_writes::machine_global_write_allowed() {
        return Vec::new();
    }
    registry_dir()
        .map(|root| lock_gone_at(&root, crate::paths::codeterm_instance_id()))
        .unwrap_or_default()
}

fn lock_gone_at(root: &Path, own_id: &str) -> Vec<GoneInstance> {
    if !real_directory(root) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".lock")?;
            if id == own_id || !canonical_instance_id(id) || !regular_file(&entry.path()) {
                return None;
            }
            let mut lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(entry.path())
                .ok()?;
            lock.try_lock().ok()?;
            let mut contents = String::new();
            lock.rewind().ok()?;
            lock.read_to_string(&mut contents).ok()?;
            let instance_id = registered_instance(&name, &contents)?;
            Some(GoneInstance {
                instance_id,
                _lock: lock,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
