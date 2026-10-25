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
        let resolved_name = definition.get("name").unwrap().as_str().unwrap();
        assert!(
            resolved_name.chars().count() <= 40,
            "{name}: {resolved_name}"
        );
        assert!(!resolved_name.contains("private-test-token"));
        assert!(!resolved_name.contains("private-fragment"));
        if let Some(steps) = definition
            .get("steps")
            .and_then(serde_json::Value::as_array)
        {
            assert!(
                steps.iter().any(|step| step["routes"]
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
fn native_recipe_names_are_stable_and_inputs_stay_data() {
    let raw = authoring::source("http-health").unwrap();
    let a = blueprint_document::check(
        "http-health",
        &raw,
        &["url=https://first.example/health?token=secret".into()],
    )
    .definition
    .unwrap();
    let b = blueprint_document::check(
        "http-health",
        &raw,
        &["url=https://second.example/health".into()],
    )
    .definition
    .unwrap();
    assert_eq!(a["name"], b["name"]);
    assert!(!a["name"].as_str().unwrap().contains("secret"));
    assert_ne!(a["steps"][0]["with"]["url"], b["steps"][0]["with"]["url"]);
}
