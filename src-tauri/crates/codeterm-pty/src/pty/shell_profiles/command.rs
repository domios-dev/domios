use super::ShellProfile;
use base64::Engine;

impl ShellProfile {
    pub fn utility_command(&self, command: &str) -> Result<Option<Vec<u8>>, String> {
        if matches!(self.id.as_str(), "powershell" | "pwsh") {
            return super::powershell_utility_command(command);
        }
        if self.wsl_distro.is_some() || self.id == "git-bash" {
            let encoded = base64::engine::general_purpose::STANDARD.encode(command.as_bytes());
            return Ok(Some(
                format!("printf %s {encoded} | base64 -d | bash\r").into_bytes(),
            ));
        }
        if self.id == "cmd" {
            let path =
                std::env::temp_dir().join(format!("codeterm-shell-{}.cmd", uuid::Uuid::new_v4()));
            std::fs::write(&path, cmd_script(command)).map_err(|error| error.to_string())?;
            return Ok(Some(format!("call \"{}\"\r", path.display()).into_bytes()));
        }
        Ok(None)
    }
}

fn cmd_script(command: &str) -> String {
    format!("@echo off\r\n{command}\r\nset \"_ct_exit=%errorlevel%\"\r\n(goto) 2>nul & del \"%~f0\" & exit /b %_ct_exit%\r\n")
}

#[cfg(test)]
mod tests;
