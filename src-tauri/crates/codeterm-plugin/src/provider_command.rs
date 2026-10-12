#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn command(binary: &Path) -> Result<Command, String> {
    #[cfg(unix)]
    {
        let mut head = Vec::new();
        if let Ok(file) = std::fs::File::open(binary) {
            let _ = file.take(4096).read_to_end(&mut head);
        }
        if let Some(interpreter) = env_interpreter(&head) {
            let probe = crate::js_runtime::probe_bin(interpreter);
            let crate::js_runtime::BinResolution::Found(path) = probe.resolution else {
                return Err(format!(
                    "Cannot launch {binary:?}: shebang interpreter {interpreter:?} is unavailable; searched {:?}. Install the interpreter or a native CLI entry point.",
                    probe.searched
                ));
            };
            return Ok(interpreted_command(binary, &path));
        }
    }
    Ok(Command::new(binary))
}

#[cfg(any(unix, test))]
fn env_interpreter(head: &[u8]) -> Option<&str> {
    let line = head.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.strip_prefix("#!")?.split_whitespace();
    if words.next()? != "/usr/bin/env" {
        return None;
    }
    let interpreter = words.next()?;
    if words.next().is_some()
        || interpreter.starts_with('-')
        || interpreter.contains(['/', '\\', '='])
    {
        return None;
    }
    Some(interpreter)
}

#[cfg(any(unix, test))]
fn interpreted_command(binary: &Path, interpreter: &Path) -> Command {
    let mut command = Command::new(interpreter);
    command.arg(binary);
    command
}

pub(crate) fn versioned_bin_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut versions: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = semver::Version::parse(name.to_str()?.strip_prefix('v')?).ok()?;
            entry
                .file_type()
                .ok()?
                .is_dir()
                .then_some((version, entry.path().join("bin")))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests;
