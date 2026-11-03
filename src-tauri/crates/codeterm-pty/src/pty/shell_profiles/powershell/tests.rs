use super::*;

fn staged_path(root: &Path) -> std::path::PathBuf {
    let paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    paths[0].clone()
}

#[test]
fn multiline_body_is_private_and_byte_exact_while_dispatch_is_one_short_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("q7'k9");
    for separator in ["\n", "\r", "\r\n"] {
        let body = format!("q7'雪$()`\"{separator}{}", "z8$v4'\"\n".repeat(1600));
        let line =
            String::from_utf8(stage_command(&root, &body, SystemTime::now()).unwrap()).unwrap();
        let path = staged_path(&root);
        assert_eq!(std::fs::read(&path).unwrap(), body.as_bytes());
        assert!(codeterm_util::private_file::owner_only(&path).unwrap());
        assert!(line.ends_with('\r'));
        assert!(!line[..line.len() - 1].contains(['\r', '\n']));
        assert!(!line.contains("z8$v4"));
        assert!(line.contains("q7''k9"));
        assert!(line.len() < 1024);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn single_line_and_literal_escape_sequences_keep_the_existing_dispatch() {
    for command in ["q7v2", r"q7\nv4\r\t", "q7\tv4"] {
        assert_eq!(powershell_utility_command(command).unwrap(), None);
    }
}

#[test]
fn sweep_removes_only_expired_owned_files_with_bounded_work() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let old = now - STAGED_COMMAND_RETENTION;
    let foreign = dir.path().join("foreign.ps1");
    std::fs::write(&foreign, "q7v2").unwrap();
    let mut recent = None;
    for index in 0..SWEEP_LIMIT + 3 {
        let path = dir
            .path()
            .join(format!("command-{}.ps1", uuid::Uuid::new_v4()));
        std::fs::write(&path, "m9r4").unwrap();
        if index == 0 {
            recent = Some(path);
        } else {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
    }
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    sweep_commands(dir.path(), now);
    let after = std::fs::read_dir(dir.path()).unwrap().count();
    assert!(before - after > 0);
    assert!(before - after <= SWEEP_LIMIT);
    assert!(after >= 2);
    assert!(foreign.exists());
    assert!(recent.unwrap().exists());
}

#[test]
fn staging_refuses_a_path_with_terminal_line_breaks_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("q7\nv4");
    assert!(stage_command(&root, "q7\nv4", SystemTime::now()).is_err());
    assert!(!root.exists());
}

#[cfg(windows)]
#[test]
fn powershell_evaluates_in_current_scope_and_deletes_before_exit_or_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    for (body, suffix, expected, exit) in [
        (
            "$q7v2 = 'm9r4\n雪$()''`'",
            "; [Console]::Write($q7v2)",
            "m9r4\n雪$()'`",
            0,
        ),
        ("exit 7\n", "", "", 7),
        ("'q7v2\n", "", "", 1),
    ] {
        let line =
            String::from_utf8(stage_command(dir.path(), body, SystemTime::now()).unwrap()).unwrap();
        let path = staged_path(dir.path());
        let mut command = std::process::Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); {}{suffix}",
                line.trim_end_matches('\r')
            ),
        ]);
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(exit));
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        assert!(!path.exists());
    }
}
