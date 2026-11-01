use super::*;

#[test]
fn git_versions_parse_vendor_suffixes_and_reject_invalid_output() {
    for (text, expected) in [
        (
            "git version 2.21.0 (Apple Git-122)\n",
            Some(GitVersion(2, 21, 0)),
        ),
        ("git version 2.25.0", Some(GitVersion(2, 25, 0))),
        ("git version 2.51.1.windows.1", Some(GitVersion(2, 51, 1))),
        ("git version 3.0.0", Some(GitVersion(3, 0, 0))),
        ("git version 2.no.0", None),
        ("git version 2.25", None),
        ("unavailable", None),
    ] {
        assert_eq!(GitVersion::parse(text), expected, "{text}");
    }
}

#[test]
fn newest_usable_git_wins_and_ties_keep_candidate_order() {
    let candidates = [
        "/usr/bin/git",
        "/usr/local/bin/git",
        "/opt/homebrew/bin/git",
        "/missing/git",
    ]
    .map(PathBuf::from);
    let selected = newest_git(candidates, |path| match path.to_str().unwrap() {
        "/usr/bin/git" => Some(GitVersion(2, 21, 0)),
        "/usr/local/bin/git" | "/opt/homebrew/bin/git" => Some(GitVersion(2, 50, 0)),
        _ => None,
    });
    assert_eq!(selected.path, Path::new("/usr/local/bin/git"));
    assert_eq!(selected.version, Some(GitVersion(2, 50, 0)));
    let missing = newest_git([PathBuf::from("/missing/git")], |_| None);
    assert_eq!(missing.version, None);
}

#[test]
fn apple_git_clone_uses_only_shallow_clone_fetch_and_checkout() {
    let source =
        crate::util::parse_channel_source("https://example.invalid/repo.git#master:plugins");
    let apple = GitVersion::parse("git version 2.21.0 (Apple Git-122)").unwrap();
    let commands = clone_commands(&source, Path::new("scratch"), apple >= GitVersion(2, 25, 0));
    assert_eq!(
        commands,
        vec![
            vec![
                "clone",
                "-c",
                "core.longpaths=true",
                "--depth",
                "1",
                "https://example.invalid/repo.git",
                "scratch"
            ],
            vec!["fetch", "--depth", "1", "origin", "master"],
            vec!["checkout", "FETCH_HEAD"],
        ]
    );
}

#[test]
fn modern_git_keeps_sparse_clone_and_subdirectory_checkout() {
    let source =
        crate::util::parse_channel_source("https://example.invalid/repo.git#master:plugins");
    let commands = clone_commands(&source, Path::new("scratch"), true);
    assert!(commands[0].contains(&"--sparse".into()));
    assert!(commands[0].contains(&"--filter=blob:none".into()));
    assert_eq!(commands[3], ["sparse-checkout", "set", "--cone", "plugins"]);
    let source = crate::util::parse_channel_source("https://example.invalid/repo.git");
    let commands = clone_commands(&source, Path::new("scratch"), false);
    assert_eq!(commands.len(), 1);
    assert!(!commands[0].contains(&"--sparse".into()));
}
