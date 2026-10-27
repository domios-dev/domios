// WHAT: native interactive-login Bash resolves both instance CLI names after PATH replacement.
// WHY: login shells ignore rcfile and BASH_ENV integration.
// ORACLE: native profile order is unchanged; both names preserve arguments, status and shell lifetime.
use super::*;
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

struct BashChild(Option<Child>);

impl Drop for BashChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            // SAFETY: this unreaped child owns the process group created for the fixture.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            let _ = child.wait();
        }
    }
}

fn run_bash(cmd: &CommandBuilder, root: &std::path::Path) -> String {
    let stdout = root.join("stdout");
    let stderr = root.join("stderr");
    let mut command = Command::new("/bin/bash");
    if cmd.is_default_prog() {
        command.arg0("-bash");
    } else {
        command.args(&cmd.get_argv()[1..]);
    }
    command
        .args(["-i", "-c", LOGIN_COMMAND])
        .env_clear()
        .envs(cmd.iter_full_env_as_str())
        .current_dir(root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    let mut child = BashChild(Some(command.spawn().expect("native /bin/bash must be available")));
    let status = child.0.as_mut().unwrap().wait_timeout(Duration::from_secs(10)).unwrap();
    let status = match status {
        Some(status) => {
            let _ = child.0.take();
            status
        }
        None => panic!("native Bash fixture exceeded its failure deadline"),
    };
    assert!(
        status.success(),
        "native Bash failed: {}",
        std::fs::read_to_string(stderr).unwrap()
    );
    std::fs::read_to_string(stdout).unwrap()
}

const LOGIN_COMMAND: &str = r#"
shopt -q login_shell || exit 91
[[ $- == *i* ]] || exit 92
printf 'command\n'
domios 'space value' '' 'literal $HOME;*'
printf '|domios-status=%s\n' "$?"
codeterm 'space value' '' 'literal $HOME;*'
printf '|codeterm-status=%s\n' "$?"
printf 'shell-alive\n'
exit 0
"#;

#[test]
fn native_login_bash_keeps_profile_order_and_binds_both_cli_names() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let own = root.path().join("own 'bin");
    let foreign = root.path().join("foreign bin");
    for dir in [&home, &own, &foreign] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for (dir, marker) in [(&own, "owner"), (&foreign, "foreign")] {
        for name in ["domios", "codeterm"] {
            let binary = dir.join(name);
            std::fs::write(
                &binary,
                format!("#!/bin/sh\nprintf '{marker}'\nprintf '<%s>' \"$@\"\nexit 7\n"),
            )
            .unwrap();
            std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    std::fs::write(
        home.join(".bashrc"),
        "printf 'bashrc\\n'\nexport PATH=\"$FIXTURE_FOREIGN:/usr/bin:/bin\"\nPROMPT_COMMAND=':'\n",
    )
    .unwrap();
    std::fs::write(home.join(".bash_logout"), "printf 'logout\\n'\n").unwrap();
    let profiles = [".bash_profile", ".bash_login", ".profile"];
    for profile in profiles {
        std::fs::write(
            home.join(profile),
            format!(
                "printf '{profile}:before\\n'\n. \"$HOME/.bashrc\"\nprintf '{profile}:after\\n'\n"
            ),
        )
        .unwrap();
    }
    for profile in profiles {
        for default_prog in [true, false] {
            let mut cmd = if default_prog {
                CommandBuilder::new_default_prog()
            } else {
                let mut cmd = CommandBuilder::new("/bin/bash");
                cmd.arg("--login");
                cmd
            };
            cmd.env_clear();
            cmd.env("HOME", &home);
            cmd.env("SHELL", "/bin/bash");
            cmd.env("PATH", &foreign);
            cmd.env("FIXTURE_FOREIGN", &foreign);
            let baseline = run_bash(&cmd, root.path());
            let prefix = format!("{profile}:before\nbashrc\n{profile}:after\ncommand\n");
            let results = "<space value><><literal $HOME;*>";
            assert_eq!(
                baseline,
                format!("{prefix}foreign{results}|domios-status=7\nforeign{results}|codeterm-status=7\nshell-alive\nlogout\n")
            );
            let args = cmd.get_argv().clone();
            cmd.env("CODETERM_CLI_PATH", own.join("codeterm"));
            apply_launch_overrides(&mut cmd, None, &[]);
            assert_eq!(cmd.get_argv(), &args);
            assert_eq!(
                run_bash(&cmd, root.path()),
                format!("{prefix}owner{results}|domios-status=7\nowner{results}|codeterm-status=7\nshell-alive\nlogout\n"),
                "profile={profile}, default_prog={default_prog}"
            );
        }
        std::fs::remove_file(home.join(profile)).unwrap();
    }
}

#[test]
fn cli_binding_uses_final_launch_environment_and_discards_inherited_functions() {
    for path in [None, Some("relative/codeterm"), Some("/own bin/codeterm")] {
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env_clear();
        for name in ["domios", "codeterm"] {
            cmd.env(format!("BASH_FUNC_{name}%%"), "() { foreign; }");
        }
        if let Some(path) = path {
            cmd.env("CODETERM_CLI_PATH", path);
        }
        apply_launch_overrides(&mut cmd, None, &[]);
        for name in ["domios", "codeterm"] {
            assert_eq!(
                cmd.get_env(format!("BASH_FUNC_{name}%%")).is_some(),
                path == Some("/own bin/codeterm")
            );
        }
        assert!(cmd.is_default_prog());
        apply_launch_overrides(&mut cmd, None, &["CODETERM_CLI_PATH".into()]);
        for name in ["domios", "codeterm"] {
            assert!(cmd.get_env(format!("BASH_FUNC_{name}%%")).is_none());
        }
    }
}
