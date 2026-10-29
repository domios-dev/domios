use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{process_table, OwnedProcessTree};

#[derive(Clone)]
pub struct ProcessRecord {
    pub created: u64,
    pub parent_pid: u32,
    pub executable: Option<PathBuf>,
}

pub fn same_windows_path(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        let value = path.to_string_lossy().replace('/', "\\");
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(&value)
            .to_ascii_lowercase()
    }
    normalized(left) == normalized(right)
}

pub fn is_owned_descendant(
    root: (u32, u64),
    subject: (u32, u64),
    images: &[PathBuf],
    processes: &HashMap<u32, ProcessRecord>,
) -> bool {
    let (mut pid, mut created) = subject;
    for _ in 0..processes.len() {
        let Some(process) = processes.get(&pid) else {
            return false;
        };
        if process.created != created {
            return false;
        }
        if !process
            .executable
            .as_ref()
            .is_some_and(|image| images.iter().any(|owned| same_windows_path(image, owned)))
        {
            return false;
        }
        if (pid, created) == root {
            return true;
        }
        let Some(parent) = processes.get(&process.parent_pid) else {
            return false;
        };
        if parent.created >= created {
            return false;
        }
        pid = process.parent_pid;
        created = parent.created;
    }
    false
}
