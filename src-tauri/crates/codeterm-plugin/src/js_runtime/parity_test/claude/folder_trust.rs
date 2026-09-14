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
}
