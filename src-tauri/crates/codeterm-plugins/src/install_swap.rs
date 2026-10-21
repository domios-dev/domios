use std::io;
use std::path::Path;

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
    retryable: impl Fn(&io::Error) -> bool,
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
        perform(Step::Cleanup).map_err(|e| format!("{}: {e}", Step::Cleanup.label()))?;
    }
    Ok(())
}

fn windows_sharing_error(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

pub(super) fn activate(staging: &Path, destination: &Path) -> Result<(), String> {
    let backup = destination.with_extension("rollback.tmp-install");
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
            Step::Activate => std::fs::rename(staging, destination),
            Step::Restore => std::fs::rename(&backup, destination),
            Step::Cleanup => std::fs::remove_dir_all(&backup),
        },
        windows_sharing_error,
    )
    .map_err(|e| {
        format!(
            "install {}: {e}; backup {}",
            destination.display(),
            backup.display()
        )
    })
}

#[cfg(test)]
mod tests;
