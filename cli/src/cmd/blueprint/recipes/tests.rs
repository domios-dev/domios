use super::super::{authoring, recipe_files};
use codeterm_blueprint_types::blueprint_document;

#[test]
fn every_shipped_recipe_checks_with_bindings_and_keeps_routes() {
    for (name, raw) in recipe_files::FILES {
        let document: blueprint_document::Document = serde_yaml::from_str(raw).unwrap();
        let assignments = document
            .inputs
            .iter()
            .filter(|(_, input)| input.default.is_none())
            .map(|(key, input)| {
                let value = if input.kind == "integer" {
                    "42"
                } else {
                    match key.as_str() {
                        "name" => "Watch target",
                        "repo" => "a-realistic-long-organization/a-realistic-long-repository",
                        "org" => "organization",
                        "url" => "https://api.example.com/v1/healthz?token=private-test-token#private-fragment",
                        "path_to_voice_note" => {
                            if cfg!(windows) {
                                "C:/voice.ogg"
                            } else {
                                "/tmp/voice.ogg"
                            }
                        }
                        "cron_name_or_id" => "nightly",
                        "tab_id" => "agent_tab",
                        key if key.ends_with("_probe") => "printf waiting # a realistic read-only shell command with arguments and enough additional detail to exceed one hundred characters",
                        _ => "ready",
                    }
                };
                format!("{key}={value}")
            })
            .collect::<Vec<_>>();
        let checked = blueprint_document::check(name, raw, &assignments);
        assert!(checked.ok, "{name}: {:?}", checked.diagnostics);
        let definition = checked.definition.unwrap();
        let resolved_name = definition
            .pointer("/spec/name")
            .or_else(|| definition.pointer("/create/name"))
            .unwrap()
            .as_str()
            .unwrap();
        assert!(
            resolved_name.chars().count() <= 40,
            "{name}: {resolved_name}"
        );
        assert!(!resolved_name.contains("private-test-token"));
        assert!(!resolved_name.contains("private-fragment"));
        if let Some(steps) = definition
            .pointer("/spec/steps")
            .and_then(serde_json::Value::as_array)
        {
            assert!(
                steps.iter().any(|step| step["on"]
                    .as_object()
                    .is_some_and(|routes| !routes.is_empty())),
                "{name}"
            );
        }
        let printed = authoring::scaffold(name).unwrap();
        assert!(printed.starts_with("# yaml-language-server: $schema="));
        assert_eq!(
            blueprint_document::check(name, &printed, &assignments).definition,
            Some(definition)
        );
    }
}

#[test]
fn custom_scaffolds_are_checkable_and_unknown_recipes_are_errors() {
    for kind in ["watch", "pipeline", "command"] {
        let raw = authoring::scaffold(kind).unwrap();
        let checked = blueprint_document::check(kind, &raw, &[]);
        assert!(checked.ok, "{kind}: {:?}", checked.diagnostics);
    }
    assert!(authoring::scaffold("unknown-recipe").is_err());
}
#[test]
fn recipe_names_follow_the_target_and_remain_bounded() {
    for (name, first, second) in [
        (
            "github",
            vec!["repo=owner/first", "id=42"],
            vec!["repo=owner/second", "id=42"],
        ),
        (
            "http-health",
            vec!["url=https://first.example.com/health"],
            vec!["url=https://second.example.com/health"],
        ),
        (
            "github-runners",
            vec!["repo=owner/first"],
            vec!["repo=owner/second"],
        ),
        (
            "github-pr-checks",
            vec!["repo=owner/first", "id=7"],
            vec!["repo=owner/first", "id=8"],
        ),
    ] {
        let raw = authoring::source(name).unwrap();
        let bind = |values: Vec<&str>| {
            blueprint_document::check(
                name,
                &raw,
                &values.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            )
        };
        let first = bind(first);
        let second = bind(second);
        assert!(
            first.ok && second.ok,
            "{name}: {:?}; {:?}",
            first.diagnostics,
            second.diagnostics
        );
        let first = first.definition.unwrap();
        let second = second.definition.unwrap();
        assert_ne!(first["spec"]["name"], second["spec"]["name"]);
        assert!(first["spec"]["name"].as_str().unwrap().chars().count() <= 40);
    }
}
#[test]
fn http_names_distinguish_paths_and_hide_query_secrets() {
    let raw = authoring::source("http-health").unwrap();
    let bind = |url: &str| {
        let checked = blueprint_document::check("http-health", &raw, &[format!("url={url}")]);
        assert!(checked.ok, "{:?}", checked.diagnostics);
        checked.definition.unwrap()
    };
    let first_url = "https://api.example.com/v1/healthz?token=secret-one#private";
    let first = bind(first_url);
    let second = bind("https://api.example.com/v2/healthz?token=secret-two#private");
    let name = first["spec"]["name"].as_str().unwrap();
    for secret in ["user", "password", "secret-one", "private", "token="] {
        assert!(!name.contains(secret), "{name}");
    }
    assert!(name.chars().count() <= 40);
    assert_ne!(first["spec"]["name"], second["spec"]["name"]);
    let rotated = bind("https://api.example.com/v1/healthz?token=rotated#different");
    assert_eq!(first["spec"]["name"], rotated["spec"]["name"]);
    assert_eq!(
        first.pointer("/spec/steps/0/kind/probe/url").unwrap(),
        first_url
    );
    assert_eq!(first["spec"]["name"], bind(first_url)["spec"]["name"]);
}
