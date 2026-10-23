use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) fn ensure_no_writer(_path: &Path, source: &Metadata) -> Result<(), String> {
    ensure_no_writer_in(Path::new("/proc"), source, |path| std::fs::read_dir(path))
}

fn inaccessible_process(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EACCES | libc::ESRCH | libc::ENOENT)
    )
}

fn ensure_no_writer_in(
    root: &Path,
    source: &Metadata,
    mut read_dir: impl FnMut(&Path) -> std::io::Result<std::fs::ReadDir>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let processes = read_dir(root).map_err(|e| e.to_string())?;
    for process in processes {
        if Instant::now() >= deadline {
            return Err("Attachment writer inspection exceeded its deadline".into());
        }
        let process = match process {
            Ok(process) => process,
            Err(error) if inaccessible_process(&error) => continue,
            Err(error) => return Err(error.to_string()),
        };
        if process
            .file_name()
            .to_string_lossy()
            .parse::<u32>()
            .is_err()
        {
            continue;
        }
        let Ok(owner) = process.metadata() else {
            continue;
        };
        if owner.uid() != unsafe { libc::geteuid() } {
            continue;
        }
        let descriptors = match read_dir(&process.path().join("fd")) {
            Ok(descriptors) => descriptors,
            Err(error) if inaccessible_process(&error) => continue,
            Err(error) => return Err(format!("Cannot verify attachment writer handles: {error}")),
        };
        for descriptor in descriptors {
            if Instant::now() >= deadline {
                return Err("Attachment writer inspection exceeded its deadline".into());
            }
            let descriptor = match descriptor {
                Ok(descriptor) => descriptor,
                Err(error) if inaccessible_process(&error) => continue,
                Err(error) => return Err(error.to_string()),
            };
            let Ok(metadata) = std::fs::metadata(descriptor.path()) else {
                continue;
            };
            if metadata.dev() != source.dev() || metadata.ino() != source.ino() {
                continue;
            }
            let info_path = process.path().join("fdinfo").join(descriptor.file_name());
            let info = match std::fs::read_to_string(info_path) {
                Ok(info) => info,
                Err(error) if inaccessible_process(&error) => continue,
                Err(error) => {
                    return Err(format!("Cannot verify attachment writer handles: {error}"))
                }
            };
            let flags = info
                .lines()
                .find_map(|line| line.strip_prefix("flags:"))
                .and_then(|flags| u32::from_str_radix(flags.trim(), 8).ok())
                .ok_or("Cannot verify attachment writer flags")?;
            if flags & libc::O_ACCMODE as u32 != libc::O_RDONLY as u32 {
                return Err(super::WRITING.into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
