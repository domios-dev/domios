use super::ShellProfile;
use portable_pty::CommandBuilder;

pub fn apply_profile(
    cmd: &mut CommandBuilder,
    profile: &ShellProfile,
    nonce: Option<&str>,
) -> Result<(), String> {
    let dir = super::super::shell_integration::integration_dir();
    if let Some(distro) = &profile.wsl_distro {
        let dir = super::windows_to_wsl(&dir.to_string_lossy(), distro)?;
        cmd.args([
            "--exec",
            "sh",
            "-c",
            WSL_INIT,
            "codeterm",
            &dir,
            nonce.unwrap_or_default(),
        ]);
    } else if profile.id == "cmd" {
        let prompt = std::env::var("PROMPT").unwrap_or_else(|_| "$P$G".into());
        cmd.env("PROMPT", format!("$E]7;file://localhost/$P$E\\{prompt}"));
    } else if profile.id == "git-bash" {
        cmd.env("CODETERM_HOST_PATHS", "1");
        cmd.args(["--rcfile", &dir.join("bashrc").to_string_lossy(), "-i"]);
    } else {
        super::super::shell_integration::apply_env(cmd, &profile.executable);
    }
    Ok(())
}

const WSL_INIT: &str = r#"
export TERM=xterm-256color COLORTERM=truecolor CODETERM=1 TERM_PROGRAM=CodeTerm
export CODETERM_SHELL_NONCE="$2"
unset CODETERM_HOST_PATHS
shell=$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f7)
shell=${shell:-${SHELL:-/bin/sh}}
case "$shell" in
  */bash) exec "$shell" --rcfile "$1/bashrc" -i ;;
  */zsh) export CODETERM_ORIG_ZDOTDIR="${ZDOTDIR:-$HOME}" ZDOTDIR="$1"; exec "$shell" -i ;;
  */fish) export XDG_DATA_DIRS="$1:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"; exec "$shell" -i ;;
  *) export PS1='$(printf "\033]7;file://localhost%s\007" "$PWD")$ '; exec "$shell" -i ;;
esac
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_integration_keeps_distro_cwd_and_nonce_as_separate_arguments() {
        let profile = ShellProfile {
            id: "wsl:Ubuntu".into(),
            name: "Ubuntu".into(),
            executable: "wsl.exe".into(),
            args: vec!["--distribution".into(), "Ubuntu".into()],
            wsl_distro: Some("Ubuntu".into()),
        };
        let launch = profile.launch(Some("C:/repo with spaces")).unwrap();
        let mut command = CommandBuilder::new(&launch.executable);
        command.args(&launch.args);
        apply_profile(&mut command, &profile, Some("own-nonce")).unwrap();
        let args = command.get_argv();
        assert_eq!(
            &args[..5],
            [
                "wsl.exe",
                "--distribution",
                "Ubuntu",
                "--cd",
                "/mnt/c/repo with spaces"
            ]
            .map(std::ffi::OsString::from)
        );
        assert_eq!(args[5], "--exec");
        assert_eq!(args[6], "sh");
        assert_eq!(args.last().unwrap(), "own-nonce");
        assert!(command.get_cwd().is_none());
    }

    #[test]
    fn git_bash_integration_uses_interactive_rcfile_without_powershell_arguments() {
        let profile = ShellProfile {
            id: "git-bash".into(),
            name: "Git Bash".into(),
            executable: "bash.exe".into(),
            args: vec![],
            wsl_distro: None,
        };
        let mut command = CommandBuilder::new(&profile.executable);
        apply_profile(&mut command, &profile, Some("own-nonce")).unwrap();
        assert_eq!(
            command.get_env("CODETERM_HOST_PATHS"),
            Some(std::ffi::OsStr::new("1"))
        );
        assert_eq!(command.get_argv()[1], "--rcfile");
        assert_eq!(command.get_argv().last().unwrap(), "-i");
    }
    #[test]
    fn cmd_profile_reports_its_current_directory_without_startup_commands() {
        let profile = ShellProfile {
            id: "cmd".into(),
            name: "Command Prompt".into(),
            executable: "cmd.exe".into(),
            args: vec![],
            wsl_distro: None,
        };
        let mut command = CommandBuilder::new(&profile.executable);
        apply_profile(&mut command, &profile, None).unwrap();
        assert_eq!(command.get_argv().len(), 1);
        assert!(command.get_env("PROMPT").is_some());
    }
}
