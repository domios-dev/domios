use super::*;

pub(super) fn carry_user_config(installed: &Path, staged: &Path) -> Result<(), String> {
    carry_file(installed, staged, codeterm_util::plugin_config::CONFIG_FILE)?;
    if unchanged_permissions(installed, staged)? {
        carry_file(installed, staged, "grants.yaml")?;
    }
    Ok(())
}

fn unchanged_permissions(installed: &Path, staged: &Path) -> Result<bool, String> {
    let path = installed.join("plugin.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let before: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(staged.join("plugin.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    Ok(before["id"].as_str().is_some()
        && before["id"] == after["id"]
        && before["permissions"] == after["permissions"]
        && before["capabilities"]["localRecordingTrigger"].is_object()
            == after["capabilities"]["localRecordingTrigger"].is_object())
}

fn carry_file(installed: &Path, staged: &Path, name: &str) -> Result<(), String> {
    let from = installed.join(name);
    let bytes = match fs::read(&from) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("preserve {}: {e}", from.display())),
    };
    let to = staged.join(name);
    codeterm_util::private_file::write_atomic(&to, &bytes)
        .map_err(|e| format!("preserve {}: {e}", to.display()))
}

#[cfg(test)]
mod tests;
