use std::io;
use std::path::Path;

#[cfg(windows)]
mod retry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Backup,
    Activate,
    Restore,
    Cleanup,
}

impl Step {
    fn label(self) -> &'static str {
        match self {
            Self::Backup => "move previous plugin aside",
            Self::Activate => "rename staging into place",
            Self::Restore => "restore previous plugin",
            Self::Cleanup => "remove previous plugin backup",
        }
    }
}

fn swap(
    has_previous: bool,
    mut perform: impl FnMut(Step) -> io::Result<()>,
    mut retryable: impl FnMut(&io::Error) -> bool,
) -> Result<(), String> {
    if has_previous {
        perform(Step::Backup).map_err(|e| format!("{}: {e}", Step::Backup.label()))?;
    }
    let mut attempts = 0;
    let activation = loop {
        attempts += 1;
        match perform(Step::Activate) {
            Err(error) if attempts < 4 && retryable(&error) => continue,
            result => break result,
        }
    };
    if let Err(error) = activation {
        let failure = format!("{} (attempts {attempts}): {error}", Step::Activate.label());
        return Err(if has_previous {
            match perform(Step::Restore) {
                Ok(()) => format!("{failure}; previous plugin restored"),
                Err(rollback) => format!("{failure}; {}: {rollback}", Step::Restore.label()),
            }
        } else {
            failure
        });
    }
    if has_previous {
        if let Err(error) = perform(Step::Cleanup) {
            log::warn!(
                "[plugin] {}: {error}; startup will clean the backup",
                Step::Cleanup.label()
            );
        }
    }
    Ok(())
}

fn windows_sharing_error(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

pub(super) fn activate(staging: &Path, destination: &Path) -> Result<(), String> {
    activate_with_reload(staging, destination, &mut || Ok(()))
}

pub(super) fn activate_with_reload(
    staging: &Path,
    destination: &Path,
    reload: &mut dyn FnMut() -> Result<(), String>,
) -> Result<(), String> {
    #[cfg(windows)]
    let retry = retry::ChangeRetry::new(staging)
        .map_err(|error| format!("watch locked staging path {}: {error}", staging.display()))?;
    let backup = super::staging::path_for(destination, "rollback.tmp-install");
    super::ensure_plugin_path_inside(&backup)?;
    if backup
        .try_exists()
        .map_err(|e| format!("inspect backup {}: {e}", backup.display()))?
    {
        return Err(format!(
            "previous plugin backup already exists: {}",
            backup.display()
        ));
    }
    let previous = destination
        .try_exists()
        .map_err(|e| format!("inspect installed plugin {}: {e}", destination.display()))?;
    swap(
        previous,
        |step| match step {
            Step::Backup => std::fs::rename(destination, &backup),
            Step::Activate => {
                std::fs::rename(staging, destination)?;
                if let Err(error) = reload() {
                    std::fs::rename(destination, staging).map_err(|rollback| {
                        io::Error::other(format!(
                            "register new plugin: {error}; withdraw new plugin: {rollback}"
                        ))
                    })?;
                    return Err(io::Error::other(format!("register new plugin: {error}")));
                }
                Ok(())
            }
            Step::Restore => std::fs::rename(&backup, destination),
            Step::Cleanup => std::fs::remove_dir_all(&backup),
        },
        |error| {
            if !windows_sharing_error(error) {
                return false;
            }
            #[cfg(windows)]
            return retry.changed(staging, destination);
            #[cfg(not(windows))]
            false
        },
    )
    .map_err(|e| {
        format!(
            "install {} from staging {}: {e}; a sharing violation may lock either path; backup {}",
            destination.display(),
            staging.display(),
            backup.display()
        )
    })
}

#[cfg(test)]
mod tests;
