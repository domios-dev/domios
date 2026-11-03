use std::path::Path;
use std::time::{Duration, SystemTime};

const SWEEP_LIMIT: usize = 64;
const STAGED_COMMAND_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

pub fn powershell_utility_command(command: &str) -> Result<Option<Vec<u8>>, String> {
    if !command.contains(['\r', '\n']) {
        return Ok(None);
    }
    let root = codeterm_paths::data_dir().join("runtime/shell-commands");
    stage_command(&root, command, SystemTime::now()).map(Some)
}

fn stage_command(root: &Path, command: &str, now: SystemTime) -> Result<Vec<u8>, String> {
    sweep_commands(root, now);
    let path = root.join(format!("command-{}.ps1", uuid::Uuid::new_v4()));
    let path_text = path.to_string_lossy();
    if path_text.contains(['\r', '\n']) {
        return Err("PowerShell command staging path must be a single line".into());
    }
    codeterm_util::private_file::write_atomic(&path, command.as_bytes())
        .map_err(|error| format!("PowerShell command staging failed: {error}"))?;
    let quoted = path_text.replace('\'', "''");
    Ok(format!(
        "Invoke-Expression (& {{ try {{ [System.IO.File]::ReadAllText('{quoted}') }} finally {{ [System.IO.File]::Delete('{quoted}') }} }})\r"
    )
    .into_bytes())
}

fn sweep_commands(root: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.take(SWEEP_LIMIT).flatten() {
        let name = entry.file_name();
        let owned = name
            .to_str()
            .and_then(|name| name.strip_prefix("command-"))
            .and_then(|name| name.strip_suffix(".ps1"))
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
        if !owned || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STAGED_COMMAND_RETENTION);
        if expired {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
