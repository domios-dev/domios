use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const REGISTRATION: &str = "codeterm-instance-lifetime-v1";
const PREDECESSOR_EXIT_DEADLINE: Duration = Duration::from_secs(30);

pub struct InstanceLifetime {
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

pub fn acquire() -> io::Result<InstanceLifetime> {
    acquire_at(
        &registry_dir()?,
        crate::paths::codeterm_instance_id(),
        PREDECESSOR_EXIT_DEADLINE,
    )
}

fn acquire_at(root: &Path, id: &str, deadline: Duration) -> io::Result<InstanceLifetime> {
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
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            log::warn!("[startup] instance lifetime lock held; waiting for its holder to exit");
            lock = wait_for_release(lock, deadline)?;
        }
        Err(error) => return Err(io::Error::other(error)),
    }
    lock.set_len(0)?;
    lock.write_all(registration(id).as_bytes())?;
    lock.sync_all()?;
    Ok(InstanceLifetime { _lock: lock })
}

// App restart spawns its successor before the predecessor exits, and the OS releases
// that lock only at process exit; a peer's gone-sweep also holds it briefly.
fn wait_for_release(lock: File, deadline: Duration) -> io::Result<File> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("instance-lifetime-wait".into())
        .spawn(move || {
            let _ = sender.send(lock.lock().map(|()| lock));
        })?;
    receiver
        .recv_timeout(deadline)
        .map_err(|_| io::Error::other("instance lifetime lock still held at the deadline"))?
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
