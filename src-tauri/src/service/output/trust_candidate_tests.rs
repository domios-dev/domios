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
    let claude_fixtures = [
        "folder-trust/quick-safety-yes-focused.txt",
        "folder-trust/trust-rule-focus-affirmative.txt",
        "folder-trust/trust-numbered-focus-affirmative.txt",
    ];

    assert!(may_be_directory_trust_prompt(&codex));
    for name in claude_fixtures {
        assert!(
            may_be_directory_trust_prompt(&fixture("claude", name)),
            "{name}"
        );
    }
}

#[test]
fn update_dialog_does_not_reach_the_directory_trust_classifier() {
    let update = fixture("codex", "directory-trust/codex-update-available.txt");
    assert!(!may_be_directory_trust_prompt(&update));
}
