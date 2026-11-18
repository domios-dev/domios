use super::*;

#[test]
fn git_command_uses_platform_arguments_before_subcommand() {
    let mut command = git_command();
    command.args(["worktree", "add", "-b", "lane/example", "deep path", "HEAD"]);
    let actual: Vec<_> = command.get_args().collect();
    #[cfg(windows)]
    let expected = [
        "-c",
        "core.longpaths=true",
        "worktree",
        "add",
        "-b",
        "lane/example",
        "deep path",
        "HEAD",
    ];
    #[cfg(not(windows))]
    let expected = ["worktree", "add", "-b", "lane/example", "deep path", "HEAD"];
    assert_eq!(command.get_program(), "git");
    assert_eq!(actual, expected);
    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn git_diagnostics_retains_errors_between_progress_updates() {
    let stderr = b"Preparing worktree (new branch 'lane/example')\nUpdating files:  10% (1/10)\rerror: unable to create file deep/file: Filename too long\nUpdating files:  90% (9/10)\rUpdating files: 100% (10/10), done.\nfatal: checkout failed\r\n";
    assert_eq!(
        git_diagnostics(stderr),
        "Preparing worktree (new branch 'lane/example')\nerror: unable to create file deep/file: Filename too long\nfatal: checkout failed"
    );
}

#[test]
fn git_diagnostics_preserves_nonprogress_percentages_and_unicode() {
    let stderr = "error: unable to create file 日本語/100% (1/2): Filename too long\r\nwarning: usage above 90%\n";
    assert_eq!(
        git_diagnostics(stderr.as_bytes()),
        stderr.trim().replace("\r\n", "\n")
    );
    assert_eq!(git_diagnostics(b""), "");
}
