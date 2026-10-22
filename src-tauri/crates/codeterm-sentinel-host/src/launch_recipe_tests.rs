use std::collections::BTreeMap;

use codeterm_types::pty::PromptKind;

use super::*;
use crate::machine::regex_verdict;
use codeterm_service_core::multiplexer::types::{PendingPrompt, PromptOption};

const RECIPE: &str = include_str!("../../../../resources/blueprints/launch-prompt-gate.yaml");

fn recipe_matches() -> BTreeMap<String, String> {
    let checked = codeterm_blueprint_types::blueprint_document::check(
        "launch-prompt-gate",
        RECIPE,
        &["cron_name_or_id=nightly".into()],
    );
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
        &checked.definition.unwrap(),
    )
    .unwrap();
    serde_json::from_value(document.lower().unwrap()["create"]["matches"].clone()).unwrap()
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
