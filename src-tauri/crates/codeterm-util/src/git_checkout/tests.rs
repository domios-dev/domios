use super::*;

#[test]
fn git_checkout_parser_version_fixtures() {
    let root = std::env::current_dir().unwrap();
    let absolute = root.join("repo/.git");
    let worktree = root.join("repo/.git/worktrees/lane");
    let fixtures = [
        (
            "git version 2.53.0",
            format!("{}\n{}\n", absolute.display(), absolute.display()),
            absolute.clone(),
            absolute.clone(),
        ),
        (
            "git version 2.21.0 (Apple Git-122)",
            ".git\n.git\n".into(),
            root.join(".git"),
            root.join(".git"),
        ),
        (
            "git version 2.21.0 (Apple Git-122)",
            format!("{}\n{}\n", worktree.display(), absolute.display()),
            worktree.clone(),
            absolute.clone(),
        ),
        (
            "git version 2.53.0",
            "../repo/.git/worktrees/lane\r\n../repo/.git\r\n".into(),
            root.join("../repo/.git/worktrees/lane"),
            root.join("../repo/.git"),
        ),
    ];
    for (version, stdout, git_dir, common_dir) in fixtures {
        assert_eq!(
            parse_paths(&root, stdout.as_bytes(), || version.into()).unwrap(),
            (git_dir, common_dir)
        );
    }
}

#[test]
fn git_checkout_parser_rejects_unexpected_output_with_version() {
    let version = "git version 2.21.0 (Apple Git-122)";
    for stdout in [
        "--path-format=absolute\n.git\n.git\n",
        "--path-format=absolute\n.git\n",
        ".git\n--git-common-dir\n",
        ".git\n",
        ".git\n.git\nextra\n",
        ".git\n\n.git\n",
        ".git\n\n",
        "",
    ] {
        let error =
            parse_paths(Path::new("repo"), stdout.as_bytes(), || version.into()).unwrap_err();
        assert!(
            matches!(&error, GitCheckoutError::UnexpectedOutput { git_version, .. } if git_version == version)
        );
        assert!(error.to_string().contains(version));
    }
    assert!(parse_paths(Path::new("repo"), b"\xff\n.git\n", || version.into()).is_err());
}

#[test]
fn git_checkout_paths_canonicalize_relative_common_directory() {
    let temp = tempfile::tempdir().unwrap();
    let common = temp.path().join("repo/.git");
    let registration = common.join("worktrees/lane");
    std::fs::create_dir_all(&registration).unwrap();
    let cwd = temp.path().join("lane");
    std::fs::create_dir(&cwd).unwrap();
    let parsed = parse_output(&cwd, b"../repo/.git/worktrees/lane\n../repo/.git\n").unwrap();
    assert_eq!(
        parsed,
        (
            registration.canonicalize().unwrap(),
            common.canonicalize().unwrap()
        )
    );
}

#[test]
fn git_checkout_command_uses_portable_flags() {
    let command = command(Path::new("repo"));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["rev-parse", "--git-dir", "--git-common-dir"]
    );
}
