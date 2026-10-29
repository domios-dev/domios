use crate::service::ServiceContext;
use std::path::Path;
use crate::service::tab_credentials::CredentialFile;

pub(super) struct CredentialCommand {
    command: String,
    file: Option<CredentialFile>,
    tab_id: String,
    nonce: String,
}

impl CredentialCommand {
    pub(super) fn send(&mut self, service: &ServiceContext, pty_id: &str) -> Result<(), String> {
        let generation = service.pty.lock().get_session(pty_id)
            .ok_or("credential launch PTY is absent before write")?.generation;
        let payload = crate::service::pty_io::build_launch_dispatch(&self.command)
            .map_err(|error| error.to_string())?;
        let pty = service.pty.lock();
        if !pty.session_generation_matches(pty_id, generation) {
            return Err("credential launch PTY changed before write".into());
        }
        self.transfer(service, pty_id)?;
        pty.write(pty_id, &payload)
    }

    fn transfer(&mut self, service: &ServiceContext, pty_id: &str) -> Result<(), String> {
        if let Some(file) = self.file.take() {
            service.retain_launch_credential_file(&self.tab_id, &self.nonce, pty_id, file)?;
        }
        Ok(())
    }
}

pub(super) fn wrap_command(
    service: &ServiceContext,
    tab_id: &str,
    command: &str,
    nonce: &str,
    attempt: &str,
    direct: bool,
) -> Result<CredentialCommand, String> {
    let command = super::command_with_binding_nonce(command, nonce, attempt);
    let file = if direct { None } else { Some(credential_file(service, tab_id, nonce)?) };
    Ok(CredentialCommand {
        command: file.as_ref().map_or(command.clone(), |file| with_credential_file(&command, file.path(), cfg!(windows))),
        file, tab_id: tab_id.into(), nonce: nonce.into(),
    })
}

fn credential_file(
    service: &ServiceContext,
    tab_id: &str,
    nonce: &str,
) -> Result<CredentialFile, String> {
    let credential = service
        .mint_tab_credential(tab_id, nonce)
        .ok_or("launch changed before credential handoff")?;
    let path = service.ensure_agent_pad(tab_id)?.path().join(format!(
        ".launch-credential-{}.txt",
        uuid::Uuid::new_v4()
    ));
    codeterm_util::private_file::write_atomic(&path, credential.as_bytes())
        .map_err(|error| format!("launch credential handoff failed: {error}"))?;
    Ok(CredentialFile(path))
}

fn with_credential_file(command: &str, path: &Path, windows: bool) -> String {
    let path = path.to_string_lossy();
    if windows {
        let path = path.replace('\'', "''");
        format!("& {{ Set-PSDebug -Off; $ErrorActionPreference='Stop'; $env:CODETERM_API_TOKEN=[System.IO.File]::ReadAllText('{path}'); if ([string]::IsNullOrWhiteSpace($env:CODETERM_API_TOKEN)) {{ throw 'launch credential file is empty' }}; Remove-Item -LiteralPath '{path}'; {command} }}")
    } else {
        let path = path.replace('\'', "'\"'\"'");
        format!("case $- in *x*) set +x; _domios_restore_trace=1;; *) _domios_restore_trace=0;; esac; if CODETERM_API_TOKEN=$(cat '{path}') && test -n \"$CODETERM_API_TOKEN\" && export CODETERM_API_TOKEN && rm -f -- '{path}'; then if [ \"$_domios_restore_trace\" = 1 ]; then set -x; fi; {command}\nelse if [ \"$_domios_restore_trace\" = 1 ]; then set -x; fi; false; fi")
    }
}

#[cfg(test)]
mod tests;
