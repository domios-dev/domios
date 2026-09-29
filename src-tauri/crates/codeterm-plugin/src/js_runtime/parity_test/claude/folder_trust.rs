//! Native/JS parity for Claude's folder-trust recognizer: both runtimes read
//! the same shared screen fixtures and must reach the declared expectation.

use super::*;
use crate::provider::{AgentProvider, DirectoryTrustPromptResponse};

fn fixtures_dir() -> std::path::PathBuf {
    workspace_plugins_root()
        .join("provider-claude")
        .join("tests")
        .join("fixtures")
        .join("folder-trust")
}

fn expected(expect: &str) -> Option<DirectoryTrustPromptResponse> {
    let answer = |step: &[u8]| {
        Some(DirectoryTrustPromptResponse::Answer {
            step: step.to_vec(),
        })
    };
    match expect {
        "enter" => answer(b"\r"),
        "move-down" => answer(b"j"),
        "move-up" => answer(b"k"),
        "hold" => Some(DirectoryTrustPromptResponse::Hold),
        "none" => None,
        other => panic!("unknown folder-trust expectation {other}"),
    }
}

#[test]
fn folder_trust_native_js_parity_on_shared_fixtures() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let cases: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures_dir().join("cases.json")).expect("cases.json"),
    )
    .expect("cases.json parses");
    let cases = cases.as_array().expect("case list");
    assert!(cases.len() >= 12, "every declared fixture is exercised");
    for case in cases {
        let file = case["file"].as_str().expect("file");
        let screen = std::fs::read_to_string(fixtures_dir().join(file)).expect("fixture");
        let want = expected(case["expect"].as_str().expect("expect"));
        assert_eq!(
            native.directory_trust_prompt_response(&screen),
            want,
            "native {file}"
        );
        assert_eq!(
            js.directory_trust_prompt_response(&screen),
            want,
            "js {file}"
        );
    }

    let screen = std::fs::read_to_string(fixtures_dir().join("quick-safety-no-first.txt"))
        .expect("active Claude trust fixture");
    assert!(native.supports_directory_trust_prompt_parsing());
    assert!(js.supports_directory_trust_prompt_parsing());
    assert_eq!(
        native.parse_directory_trust_prompt(&screen),
        js.parse_directory_trust_prompt(&screen)
    );
}

#[test]
fn trust_context_above_bypass_has_no_native_or_js_folder_trust_response() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let screen = std::fs::read_to_string(fixtures_dir().join("trust-mentioned-bypass-warning.txt"))
        .expect("bypass fixture");
    assert_eq!(native.directory_trust_prompt_response(&screen), None);
    assert_eq!(js.directory_trust_prompt_response(&screen), None);
}

#[test]
fn underscore_joined_trust_token_is_rejected_by_native_and_js() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let screen = std::fs::read_to_string(fixtures_dir().join("quick-safety-no-first.txt"))
        .expect("quick safety fixture");
    let underscore_screen = screen
        .lines()
        .map(|line| {
            let option = line.trim_start().trim_start_matches('❯').trim_start();
            if option
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("yes"))
            {
                line.replace("trust", "_trust")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        native.directory_trust_prompt_response(&underscore_screen),
        None
    );
    assert_eq!(js.directory_trust_prompt_response(&underscore_screen), None);
    assert_eq!(native.parse_prompt(&underscore_screen), None);
    assert_eq!(js.parse_prompt(&underscore_screen), None);
}

#[test]
fn quick_safety_prompt_and_response_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    for file in [
        "quick-safety-no-first.txt",
        "quick-safety-yes-focused.txt",
        "quick-safety-ambiguous-focus.txt",
        "quick-safety-wrapped-header.txt",
        "quick-safety-path-residue.txt",
        "quick-safety-path-in-dialog.txt",
        "trust-without-title.txt",
    ] {
        let screen = std::fs::read_to_string(fixtures_dir().join(file)).expect("fixture");
        let native_prompt = native.parse_prompt(&screen);
        let js_prompt = js.parse_prompt(&screen);
        assert_eq!(native_prompt, js_prompt, "parse_prompt {file}");
        let Some(prompt) = native_prompt else {
            assert!(
                file == "quick-safety-ambiguous-focus.txt"
                    || file == "quick-safety-path-in-dialog.txt"
                    || file == "trust-without-title.txt",
                "only ambiguous or structurally incomplete trust fixtures are held"
            );
            continue;
        };

        assert_eq!(prompt.options.len(), 2, "two choices {file}");
        assert!(prompt.options[0]
            .label
            .to_ascii_lowercase()
            .starts_with("no"));
        assert!(prompt.options[1]
            .label
            .to_ascii_lowercase()
            .starts_with("yes"));
        assert_eq!(prompt.options[0].value, "1");
        assert_eq!(prompt.options[1].value, "2");
        let native_steps = native.build_prompt_response(1, &screen);
        let js_steps = js.build_prompt_response(1, &screen);
        assert_eq!(native_steps, js_steps, "build_prompt_response {file}");
        assert_eq!(
            native_steps,
            if file == "quick-safety-yes-focused.txt" {
                vec![b"\r".to_vec()]
            } else {
                vec![b"j".to_vec(), b"\r".to_vec()]
            },
            "trust selection is reached before confirming {file}"
        );
    }
}

#[test]
fn numbered_trust_prompt_and_response_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let screen =
        std::fs::read_to_string(fixtures_dir().join("trust-numbered-focus-affirmative.txt"))
            .expect("numbered trust fixture");
    let native_prompt = native
        .parse_prompt(&screen)
        .expect("native parses numbered trust");
    let js_prompt = js
        .parse_prompt(&screen)
        .expect("plugin parses numbered trust");
    assert_eq!(native_prompt, js_prompt);
    assert_eq!(native_prompt.options.len(), 2);
    assert!(native_prompt.options[0]
        .label
        .to_ascii_lowercase()
        .starts_with("yes"));
    assert!(native_prompt.options[1]
        .label
        .to_ascii_lowercase()
        .starts_with("no"));
    let native_steps = native.build_prompt_response(0, &screen);
    let js_steps = js.build_prompt_response(0, &screen);
    assert_eq!(native_steps, js_steps);
    assert_eq!(native_steps, vec![b"\r".to_vec()]);
}
