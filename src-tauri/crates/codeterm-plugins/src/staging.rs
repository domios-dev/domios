use std::path::{Path, PathBuf};

pub(super) fn path_for(destination: &Path, suffix: &str) -> PathBuf {
    let mut name = destination.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    destination.with_file_name(name)
}

pub(super) fn with_cleanup<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let result = operation();
    let cleanup = remove(path);
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("clean staging {}: {error}", path.display())),
    }
}

fn restore_destination(path: &Path, destination_exists: bool) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_suffix(".rollback.tmp-install")?;
    super::PluginId::parse(id.to_string()).ok()?;
    (!destination_exists).then(|| path.with_file_name(id))
}

pub(super) fn cleanup_stale(root: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect plugin staging: {error}")),
    };
    let mut errors = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp-install")
        {
            continue;
        }
        let destination = restore_destination(&path, false);
        let result = match destination {
            Some(destination)
                if !destination
                    .try_exists()
                    .map_err(|error| error.to_string())? =>
            {
                std::fs::rename(&path, &destination)
                    .map_err(|error| format!("restore {}: {error}", destination.display()))
            }
            _ => remove(&path),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests;
