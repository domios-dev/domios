use std::path::Path;

pub const MARKER: &str = ".domios-container-managed";
pub const UPDATE_CODE: &str = "container_managed_update";
pub const UPDATE_MESSAGE: &str = "This installation is container-managed. Pull a newer image tag and recreate the container with the same data volume.";

pub fn marked_at(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(Path::parent)
        .is_some_and(|root| root.join(MARKER).symlink_metadata().is_ok())
}

pub fn marked() -> bool {
    std::env::current_exe().is_ok_and(|executable| marked_at(&executable))
}
