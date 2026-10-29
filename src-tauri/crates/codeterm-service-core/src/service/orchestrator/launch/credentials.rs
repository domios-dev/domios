use crate::service::ServiceContext;
use std::path::Path;

pub(super) fn wrap_command(
    service: &ServiceContext,
    tab_id: &str,
    command: &str,
    nonce: &str,
    attempt: &str,
    direct: bool,
) -> Result<String, String> {
    let command = super::command_with_binding_nonce(command, nonce, attempt);
    if direct {
        return Ok(command);
    }
    let path = credential_file(service, tab_id, nonce)?;
    Ok(with_credential_file(&command, &path, cfg!(windows)))
}

fn credential_file(
    service: &ServiceContext,
    tab_id: &str,
    nonce: &str,
) -> Result<std::path::PathBuf, String> {
    let credential = service
        .mint_tab_credential(tab_id, nonce)
        .ok_or("launch changed before credential handoff")?;
    let path = service.ensure_agent_pad(tab_id)?.path().join(format!(
        ".launch-credential-{}.txt",
        uuid::Uuid::new_v4()
    ));
    codeterm_util::private_file::write_atomic(&path, credential.as_bytes())
        .map_err(|error| format!("launch credential handoff failed: {error}"))?;
    Ok(path)
}

fn with_credential_file(command: &str, path: &Path, windows: bool) -> String {
    let path = path.to_string_lossy();
    if windows {
        let path = path.replace('\'', "''");
        format!("& {{ $ErrorActionPreference='Stop'; $env:CODETERM_API_TOKEN=[System.IO.File]::ReadAllText('{path}'); Remove-Item -LiteralPath '{path}'; {command} }}")
    } else {
        let path = path.replace('\'', "'\"'\"'");
        format!("CODETERM_API_TOKEN=$(cat '{path}') && export CODETERM_API_TOKEN && rm -f -- '{path}' && {{\n{command}\n}}")
    }
}

#[cfg(test)]
mod tests;
