use super::*;

/// Copy the installed plugin's `config.yaml` into the staged bundle that replaces it.
/// The user's copy wins over one the package ships; nothing else carries over.
pub(super) fn carry_user_config(installed: &Path, staged: &Path) -> Result<(), String> {
    let from = installed.join(codeterm_util::plugin_config::CONFIG_FILE);
    let bytes = match fs::read(&from) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("preserve {}: {e}", from.display())),
    };
    let to = staged.join(codeterm_util::plugin_config::CONFIG_FILE);
    codeterm_util::private_file::write_atomic(&to, &bytes)
        .map_err(|e| format!("preserve {}: {e}", to.display()))
}

#[cfg(test)]
mod tests;
