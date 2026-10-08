use super::ShellProfile;
use base64::Engine;

impl ShellProfile {
    pub fn utility_command(&self, command: &str) -> Result<Option<Vec<u8>>, String> {
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
mod tests {
    use super::*;

    #[test]
    fn utility_payload_is_encoded_once_without_shell_interpolation() {
        let profile = ShellProfile {
            id: "wsl:Ubuntu".into(),
            name: "Ubuntu".into(),
            executable: "wsl.exe".into(),
            args: vec![],
            wsl_distro: Some("Ubuntu".into()),
        };
        let command = "echo '$HOME';\necho \"quotes\"; exit 7";
        let bytes = profile.utility_command(command).unwrap().unwrap();
        let line = String::from_utf8(bytes).unwrap();
        let encoded = line
            .strip_prefix("printf %s ")
            .unwrap()
            .strip_suffix(" | base64 -d | bash\r")
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            command.as_bytes()
        );
    }

    #[test]
    fn cmd_script_preserves_multiline_and_command_exit() {
        let command = "echo one\r\necho two";
        let script = cmd_script(command);
        assert!(script.contains(command));
        let capture = script.find("set \"_ct_exit=%errorlevel%\"").unwrap();
        let cleanup = script.find("del \"%~f0\"").unwrap();
        assert!(capture < cleanup);
        assert!(script.ends_with("exit /b %_ct_exit%\r\n"));
    }

    #[cfg(windows)]
    #[test]
    fn cmd_utility_script_returns_command_exit_after_removing_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utility with spaces.cmd");
        std::fs::write(&path, cmd_script("echo first\r\ncmd /d /c exit 7")).unwrap();
        let mut command = std::process::Command::new("cmd.exe");
        command.args(["/d", "/c", "call"]).arg(&path);
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout).contains("first"));
        assert!(!path.exists());
    }
}
