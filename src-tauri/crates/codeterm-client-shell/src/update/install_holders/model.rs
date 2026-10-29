use std::collections::HashMap;
use std::path::{Path, PathBuf};

// Restart Manager keeps registrations in a size-limited registry key; foreign trees in the install folder overflow it.
pub(super) const INSTALL_BINARY_DIRS: [&str; 5] =
    ["", "cli", "domios-server", "conpty/x64", "conpty/arm64"];

pub(super) fn install_binaries(
    root: &Path,
    listing: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathBuf> {
    listing
        .into_iter()
        .filter(|path| {
            path.strip_prefix(root)
                .is_ok_and(|relative| is_install_binary(relative))
        })
        .collect()
}

fn is_install_binary(relative: &Path) -> bool {
    let binary = relative.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("dll")
    });
    let directory = relative
        .parent()
        .map(|parent| {
            parent
                .components()
                .map(|part| part.as_os_str().to_string_lossy().to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    binary && INSTALL_BINARY_DIRS.contains(&directory.as_str())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HolderDisposition {
    UpdatingApp,
    OwnedHelper,
    OwnedTab,
    #[cfg(windows)]
    OwnedServer,
    Foreign,
}

pub(super) struct Holder {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub parent_pid: Option<u32>,
    pub owned_tab: Option<String>,
}

pub(super) use codeterm_util::process_tree::{same_windows_path, ProcessRecord};

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

pub(super) fn is_attached_server(
    server: Option<&crate::application_host::AttachedServerIdentity>,
    pid: u32,
    created: u64,
) -> bool {
    server.is_some_and(|server| server.pid == pid && server.created == created.to_string())
}

pub(super) fn is_owned_server_holder(
    server: &crate::application_host::AttachedServerIdentity,
    pid: u32,
    created: u64,
    executable: Option<&Path>,
    owned_images: &[PathBuf],
    processes: &HashMap<u32, ProcessRecord>,
) -> bool {
    if !executable.is_some_and(|path| {
        owned_images
            .iter()
            .any(|owned| same_windows_path(path, owned))
    }) {
        return false;
    }
    let Ok(server_created) = server.created.parse::<u64>() else {
        return false;
    };
    codeterm_util::process_tree::is_owned_descendant(
        (server.pid, server_created),
        (pid, created),
        owned_images,
        processes,
    )
}

#[cfg(test)]
mod tests;
