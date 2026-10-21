use super::*;

#[test]
fn git_stderr_names_the_concrete_cause() {
    let cases = [
        (
            "remote: Repository not found.\nfatal: repository 'https://github.com/rollacode/codeterm.git/' not found",
            REPOSITORY_NOT_FOUND,
        ),
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            AUTHENTICATION_REQUIRED,
        ),
        (
            "fatal: unable to access 'https://github.com/o/r.git/': The requested URL returned error: 403",
            AUTHENTICATION_REQUIRED,
        ),
        (
            "git@github.com: Permission denied (publickey).",
            AUTHENTICATION_REQUIRED,
        ),
        (
            "fatal: unable to access 'https://github.com/o/r.git/': Could not resolve host: github.com",
            NETWORK_UNAVAILABLE,
        ),
        (
            "fatal: unable to access 'https://github.com/o/r.git/': Failed to connect to github.com port 443",
            NETWORK_UNAVAILABLE,
        ),
    ];
    for (stderr, cause) in cases {
        assert_eq!(git_failure_cause(stderr), cause, "{stderr}");
    }
}

#[test]
fn unknown_git_failures_keep_the_diagnostic_block() {
    assert_eq!(
        git_failure_cause("Cloning into 'x'...\nfatal: bad object HEAD\n"),
        "git failed: fatal: bad object HEAD"
    );
    assert_eq!(git_failure_cause("  \n"), "git failed");
}

#[test]
fn unavailable_message_names_cause_and_source_without_credentials() {
    let message = channel_unavailable(
        REPOSITORY_NOT_FOUND,
        "https://user:token@github.com/rollacode/codeterm.git?x=1#master:plugins",
    );
    assert_eq!(
        message,
        "Plugin channel unavailable: repository not found or not readable without access \
         (github.com/rollacode/codeterm.git). Domios will retry."
    );
    assert!(message.starts_with(CHANNEL_UNAVAILABLE_PREFIX));
    assert!(!message.contains("token"));
    assert_eq!(
        source_label("git@github.com:rollacode/codeterm.git#main"),
        "github.com:rollacode/codeterm.git"
    );
}

#[test]
fn git_recovery_hint_does_not_replace_the_error_head() {
    let stderr = "Cloning into 'plugins'...\nerror: unable to create file plugin.js: Permission denied\nfatal: unable to checkout working tree\nwarning: Clone succeeded, but checkout failed.\nand retry with 'git restore --source=HEAD :/'";
    let cause = git_failure_cause(stderr);
    assert!(
        cause.starts_with("git failed: error: unable to create file plugin.js: Permission denied")
    );
    assert!(cause.contains("fatal: unable to checkout working tree"));
    assert!(cause.ends_with("and retry with 'git restore --source=HEAD :/'"));
}
