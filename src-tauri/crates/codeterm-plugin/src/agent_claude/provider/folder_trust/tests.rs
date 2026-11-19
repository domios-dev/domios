use super::*;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(crate::test_support::fixture_manifest_dir())
        .join("../../../plugins/provider-claude/tests/fixtures/folder-trust")
}

fn expected(expect: &str) -> Option<DirectoryTrustPromptResponse> {
    let answer = |step: &[u8]| {
        Some(DirectoryTrustPromptResponse::Answer {
            step: step.to_vec(),
        })
    };
    match expect {
        "enter" => answer(b"\r"),
        "move-down" => answer(b"\x1b[B"),
        "move-up" => answer(b"\x1b[A"),
        "hold" => Some(DirectoryTrustPromptResponse::Hold),
        "none" => None,
        other => panic!("unknown folder-trust expectation {other}"),
    }
}

#[test]
fn shared_fixtures_meet_their_declared_expectations() {
    let cases: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("cases.json")).expect("cases.json"),
    )
    .expect("cases.json parses");
    let cases = cases.as_array().expect("case list");
    for case in cases {
        let file = case["file"].as_str().expect("file");
        let screen = std::fs::read_to_string(fixtures().join(file)).expect("fixture");
        assert_eq!(
            response(&screen),
            expected(case["expect"].as_str().expect("expect")),
            "{file}"
        );
    }
}

#[test]
fn transcript_quote_has_no_live_trust_prompt_or_focus() {
    let screen =
        std::fs::read_to_string(fixtures().join("claude-transcript-quotes-trust-dialog.txt"))
            .expect("quoted trust fixture");
    assert_eq!(response(&screen), None);
    assert_eq!(parse_prompt(&screen), None);
    assert_eq!(current_option_index(&screen), None);
}

#[test]
fn live_trust_dialog_accepts_trailing_blank_rows() {
    let screen = std::fs::read_to_string(fixtures().join("claude-2.1.296-trailing-blanks.txt"))
        .expect("live trust fixture with blank tail");
    assert_eq!(response(&screen), expected("move-down"));
    assert!(parse_prompt(&screen).is_some());
    assert_eq!(current_option_index(&screen), Some(0));
}

#[test]
fn enter_is_only_sent_while_the_affirmative_row_is_focused() {
    let screen = std::fs::read_to_string(fixtures().join("quick-safety-no-first.txt"))
        .expect("unfocused trust fixture");
    let affirmative = std::fs::read_to_string(fixtures().join("quick-safety-yes-focused.txt"))
        .expect("affirmative focus fixture");
    assert_eq!(
        response(&screen),
        Some(DirectoryTrustPromptResponse::Answer {
            step: b"\x1b[B".to_vec()
        })
    );
    assert_eq!(
        response(&affirmative),
        Some(DirectoryTrustPromptResponse::Answer {
            step: b"\r".to_vec()
        })
    );
}

#[test]
fn trust_context_above_bypass_is_not_a_folder_trust_prompt() {
    let screen = std::fs::read_to_string(fixtures().join("trust-mentioned-bypass-warning.txt"))
        .expect("bypass fixture");
    assert_eq!(response(&screen), None);
    assert_eq!(parse_prompt(&screen), None);
    assert_eq!(current_option_index(&screen), None);
    assert!(!has_trust_token("i_trust"));
}

#[test]
fn underscore_is_part_of_the_folder_trust_token() {
    let screen = std::fs::read_to_string(fixtures().join("quick-safety-no-first.txt"))
        .expect("trust fixture");
    let underscore_screen = screen
        .lines()
        .map(|line| {
            if leads_with(option_row(content(line)).label, "yes") {
                line.replace("trust", "_trust")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!has_trust_token("i_trust"));
    assert_eq!(response(&underscore_screen), None);
    assert_eq!(parse_prompt(&underscore_screen), None);
    assert_eq!(current_option_index(&underscore_screen), None);
}

#[test]
fn trust_dialog_without_title_holds_and_is_not_a_prompt() {
    let screen = std::fs::read_to_string(fixtures().join("trust-without-title.txt"))
        .expect("title-less trust fixture");
    assert_eq!(response(&screen), Some(DirectoryTrustPromptResponse::Hold));
    assert_eq!(parse_prompt(&screen), None);
    assert_eq!(current_option_index(&screen), None);
}

#[test]
fn continue_without_permissions_is_visible_but_not_auto_advanced() {
    let screen = std::fs::read_to_string(fixtures().join("trust-continue-without-permissions.txt"))
        .expect("continue without permissions fixture");
    assert!(parse_prompt(&screen).is_some());
    assert_eq!(response(&screen), Some(DirectoryTrustPromptResponse::Hold));
}
