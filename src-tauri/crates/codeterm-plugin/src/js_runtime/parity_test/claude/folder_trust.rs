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
fn transcript_quote_has_no_native_or_js_live_trust_prompt() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let screen =
        std::fs::read_to_string(fixtures_dir().join("claude-transcript-quotes-trust-dialog.txt"))
            .expect("quoted trust fixture");
    for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
        assert_eq!(provider.directory_trust_prompt_response(&screen), None);
        assert_eq!(provider.parse_directory_trust_prompt(&screen), None);
        assert_eq!(provider.parse_prompt(&screen), None);
    }
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

fn focus_option(screen: &str, labels: &[String], target: &str) -> String {
    screen
        .lines()
        .map(|line| {
            let normalized = line.replace('│', "").replace('┃', "").replace('║', "");
            let label = normalized
                .trim()
                .trim_start_matches(|character| character == '❯' || character == '>')
                .trim();
            if !labels.iter().any(|option| option == label) {
                return line.to_string();
            }
            let mut row = line.replace('❯', " ").replace('>', " ");
            if label == target {
                let position = row.find(label).expect("parsed option remains on screen");
                row.insert_str(position, "❯ ");
            }
            row
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn preapproved_folder_trust_selects_the_declared_option_from_live_focus() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let screen =
        std::fs::read_to_string(fixtures_dir().join("trust-local-preapproved-no-first.txt"))
            .expect("pre-approved folder trust fixture");
    let native_prompt = native
        .parse_directory_trust_prompt(&screen)
        .expect("native parses pre-approved folder trust");
    let js_prompt = js
        .parse_directory_trust_prompt(&screen)
        .expect("plugin parses pre-approved folder trust");
    assert_eq!(native_prompt, js_prompt);
    let affirmative = native_prompt
        .options
        .iter()
        .position(|option| {
            option.label.to_ascii_lowercase().starts_with("yes")
                && option.label.to_ascii_lowercase().contains("trust")
        })
        .expect("declared affirmative option");
    let down_enter = vec![b"j".to_vec(), b"\r".to_vec()];
    assert_eq!(
        native.build_prompt_response(affirmative, &screen),
        down_enter
    );
    assert_eq!(js.build_prompt_response(affirmative, &screen), down_enter);

    let labels = native_prompt
        .options
        .iter()
        .map(|option| option.label.clone())
        .collect::<Vec<_>>();
    let focused = focus_option(&screen, &labels, &native_prompt.options[affirmative].label);
    let enter = vec![b"\r".to_vec()];
    assert_eq!(native.build_prompt_response(affirmative, &focused), enter);
    assert_eq!(js.build_prompt_response(affirmative, &focused), enter);
    assert_eq!(
        native.directory_trust_prompt_response(&focused),
        Some(DirectoryTrustPromptResponse::Answer {
            step: b"\r".to_vec(),
        })
    );
    assert_eq!(
        js.directory_trust_prompt_response(&focused),
        Some(DirectoryTrustPromptResponse::Answer {
            step: b"\r".to_vec(),
        })
    );
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

type DeclaredTrustOption = (Vec<String>, Vec<u8>, Vec<Option<Vec<u8>>>);

fn declared_trust_option(
    provider: &dyn AgentProvider,
    screen: &str,
) -> Option<DeclaredTrustOption> {
    let prompt = provider.parse_directory_trust_prompt(screen)?;
    let Some(DirectoryTrustPromptResponse::Answer { step }) =
        provider.directory_trust_prompt_response(screen)
    else {
        return None;
    };
    let first_keys = (0..prompt.options.len())
        .map(|index| {
            provider
                .live_prompt_response(index, screen)
                .and_then(|steps| steps.into_iter().next())
        })
        .collect();
    Some((
        prompt
            .options
            .into_iter()
            .map(|option| option.value)
            .collect(),
        step,
        first_keys,
    ))
}

#[test]
fn parity_directory_trust_claude_declared_option_matches_native() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let cases: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures_dir().join("cases.json")).expect("cases.json"),
    )
    .expect("cases.json parses");
    let mut declared = 0;
    for case in cases.as_array().expect("case list") {
        let file = case["file"].as_str().expect("file");
        let screen = std::fs::read_to_string(fixtures_dir().join(file)).expect("fixture");
        let native_option = declared_trust_option(&native, &screen);
        assert_eq!(
            native_option,
            declared_trust_option(&js, &screen),
            "declared trust option {file}"
        );
        let Some((_, step, first_keys)) = native_option else {
            continue;
        };
        let advanced = first_keys
            .iter()
            .enumerate()
            .filter(|(_, first)| first.as_deref() == Some(step.as_slice()))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        assert_eq!(advanced.len(), 1, "one option per trust key {file}");
        let prompt = native.parse_directory_trust_prompt(&screen).unwrap();
        assert!(
            prompt.options[advanced[0]]
                .label
                .to_ascii_lowercase()
                .starts_with("yes"),
            "the trust key advances toward the affirmative option {file}"
        );
        declared += 1;
    }
    assert!(
        declared >= 3,
        "several captured trust frames declare an option"
    );
}
