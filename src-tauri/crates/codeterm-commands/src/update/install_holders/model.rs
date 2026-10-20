use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HolderDisposition {
    UpdatingApp,
    OwnedHelper,
    OwnedTab,
    Foreign,
}

pub(super) struct Holder {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub parent_pid: Option<u32>,
    pub owned_tab: Option<String>,
}

pub(super) fn same_windows_path(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        let value = path.to_string_lossy().replace('/', "\\");
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(&value)
            .to_ascii_lowercase()
    }
    normalized(left) == normalized(right)
}

pub(super) fn classify_holder(
    holder: &Holder,
    owner_pid: u32,
    installed_clis: &[PathBuf],
    child_helpers: &[PathBuf],
) -> HolderDisposition {
    if holder.pid == owner_pid {
        return HolderDisposition::UpdatingApp;
    }
    if holder.owned_tab.is_some() {
        return HolderDisposition::OwnedTab;
    }
    let Some(path) = &holder.executable else {
        return HolderDisposition::Foreign;
    };
    if installed_clis
        .iter()
        .any(|owned| same_windows_path(path, owned))
        || (holder.parent_pid == Some(owner_pid)
            && child_helpers
                .iter()
                .any(|owned| same_windows_path(path, owned)))
    {
        HolderDisposition::OwnedHelper
    } else {
        HolderDisposition::Foreign
    }
}

pub(super) fn foreign_notice(holders: &[Holder]) -> String {
    let names = holders
        .iter()
        .map(|holder| format!("{} (PID {})", holder.name, holder.pid))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "Update blocked by install-file holders: {names}. Close these processes and choose Retry."
    )
}

pub(super) fn same_process_identity(expected: (u32, u32), actual: (u32, u32)) -> bool {
    expected == actual
}

#[cfg(test)]
mod tests;
