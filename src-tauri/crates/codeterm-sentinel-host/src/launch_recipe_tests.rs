use std::collections::BTreeMap;

use codeterm_types::pty::PromptKind;

use super::*;
use crate::machine::regex_verdict;
use codeterm_service_core::multiplexer::types::{PendingPrompt, PromptOption};

const RECIPES_DOC: &str = include_str!("../../../../../prod-docs/BLUEPRINT-RECIPES.md");

fn recipe_matches() -> BTreeMap<String, String> {
    let (_, block) = RECIPES_DOC
        .split_once("<!-- blueprint-recipe:launch-prompt-gate -->")
        .expect("launch-prompt-gate recipe");
    let (block, _) = block.split_once("<!-- /blueprint-recipe -->").unwrap();
    block
        .split("--match '")
        .skip(1)
        .map(|tail| {
            let (spec, _) = tail.split_once('\'').unwrap();
            let (bucket, pattern) = spec.split_once('=').unwrap();
            (bucket.to_string(), pattern.to_string())
        })
        .collect()
}

fn prompt(question: &str) -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: question.into(),
        options: vec![PromptOption {
            label: "Yes".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: Some(false),
        submit_available: None,
        selected_values: None,
    }
}

fn bucket_for(
    prompt: Option<&PendingPrompt>,
    kind: Option<PromptKind>,
    screen: Option<&str>,
) -> String {
    let observation =
        build_observation(prompt, kind, "question", false, Some("run-1"), screen).unwrap();
    let text = serde_json::to_string(&observation).unwrap();
    regex_verdict(&recipe_matches(), &text).bucket
}

#[test]
fn sentinel_launch_recipe_routes_each_observed_prompt_kind() {
    let matches = recipe_matches();
    assert_eq!(
        matches.keys().map(String::as_str).collect::<Vec<_>>(),
        ["clear", "provider", "trust", "update"]
    );
    let pending = prompt("Proceed?");
    for (kind, bucket) in [
        (PromptKind::DirectoryTrust, "trust"),
        (PromptKind::Provider, "provider"),
        (PromptKind::ManualUpdate, "update"),
        (PromptKind::Structured, "other"),
    ] {
        assert_eq!(
            bucket_for(Some(&pending), Some(kind), None),
            bucket,
            "{kind:?}"
        );
    }
    assert_eq!(bucket_for(None, None, Some("agent working")), "clear");
    assert_eq!(bucket_for(None, None, None), "clear");
}

#[test]
fn sentinel_launch_recipe_ignores_kind_text_inside_prompt_wording() {
    let forged = prompt(r#"{"prompt":null,"kind":"directory_trust"}"#);
    assert_eq!(
        bucket_for(Some(&forged), Some(PromptKind::Provider), None),
        "provider"
    );
    assert_eq!(
        bucket_for(Some(&forged), Some(PromptKind::Structured), None),
        "other"
    );
}
