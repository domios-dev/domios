use super::may_be_directory_trust_prompt;

fn fixture(provider: &str, name: &str) -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../plugins/provider-{provider}/tests/fixtures"))
            .join(name),
    )
    .expect("dialog fixture")
}

#[test]
fn trust_candidate_prefilter_covers_codex_and_claude_dialog_frames() {
    let codex = fixture("codex", "directory-trust/trust-folder-access-0.157.1.txt");
    let live_codex = fixture("codex", "directory-trust/trust-frame-live-user-dir-0.157.1.txt");
    let claude_fixtures = [
        "folder-trust/quick-safety-yes-focused.txt",
        "folder-trust/trust-rule-focus-affirmative.txt",
        "folder-trust/trust-numbered-focus-affirmative.txt",
    ];

    assert!(may_be_directory_trust_prompt(&codex));
    assert!(may_be_directory_trust_prompt(&live_codex));
    for name in claude_fixtures {
        assert!(
            may_be_directory_trust_prompt(&fixture("claude", name)),
            "{name}"
        );
    }
}

#[test]
fn trust_candidate_skips_trailing_blank_rows_before_taking_its_tail() {
    let full_screen = fixture(
        "codex",
        "directory-trust/trust-frame-full-screen-trailing-blank-rows.txt",
    );
    assert!(full_screen.lines().count() > 24);
    assert!(full_screen
        .lines()
        .rev()
        .take(24)
        .all(|line| line.trim().is_empty()));
    assert!(may_be_directory_trust_prompt(&full_screen));
}

#[test]
fn update_dialog_does_not_reach_the_directory_trust_classifier() {
    let update = fixture("codex", "directory-trust/codex-update-available.txt");
    assert!(!may_be_directory_trust_prompt(&update));
}
