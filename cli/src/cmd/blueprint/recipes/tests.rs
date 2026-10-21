use super::*;
use serde_json::Value;

#[test]
fn blueprint_recipe_catalog_includes_echo_and_both_definition_kinds() {
    let recipes = recipes().unwrap();
    assert!(recipes.iter().any(|recipe| recipe.spec.is_none()));
    let echo = recipes.iter().find(|recipe| recipe.name == "echo").unwrap();
    assert_eq!(
        echo.spec.as_ref().unwrap()["steps"][0]["kind"]["kind"],
        "command"
    );
    assert_eq!(pipeline_spec("echo").unwrap(), *echo.spec.as_ref().unwrap());
}

#[test]
fn every_recipe_contains_handlers_and_a_dry_run_create_command() {
    use clap::Parser;

    for recipe in recipes()
        .unwrap()
        .into_iter()
        .filter(|recipe| recipe.spec.is_none())
    {
        let handlers: Value = serde_json::from_str(&recipe.handlers).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        std::fs::write(&path, &recipe.handlers).unwrap();
        let mut command = recipe
            .command
            .split_whitespace()
            .map(|part| part.trim_matches(|character| character == '\'' || character == '"'))
            .map(str::to_string)
            .collect::<Vec<_>>();
        let handlers_arg = command
            .windows(2)
            .position(|pair| pair[0] == "--handlers-file")
            .map(|index| index + 1)
            .unwrap();
        command[handlers_arg] = path.to_string_lossy().into_owned();
        assert_eq!(command.first().map(String::as_str), Some("codeterm"));
        assert_eq!(command.get(1).map(String::as_str), Some("blueprint"));
        assert_eq!(command.get(2).map(String::as_str), Some("create"));
        let buckets = command
            .windows(2)
            .find(|pair| pair[0] == "--buckets")
            .map(|pair| pair[1].split(',').map(str::to_string).collect::<Vec<_>>())
            .unwrap();
        let normalized = super::super::handlers::validate_handler_file(handlers, &buckets)
            .unwrap_or_else(|error| panic!("recipe {}: {error}", recipe.name));
        assert!(normalized.is_object());
        let cli = crate::args::cli::Cli::try_parse_from(command).unwrap();
        assert!(
            recipe.command.contains("--timeout "),
            "recipe {} omitted the composition timeout",
            recipe.name
        );
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("recipe command did not parse as blueprint create")
        };
        let crate::args::blueprint::BlueprintAction::Create { name, dry_run, .. } = &action
        else {
            panic!("recipe command did not parse as blueprint create")
        };
        assert!(*dry_run, "recipe {} omitted --dry-run", recipe.name);
        codeterm_blueprint_types::automation_name::validate_sentinel_name(
            name.as_deref().unwrap_or(""),
        )
        .unwrap_or_else(|error| panic!("recipe {}: {error}", recipe.name));
        let (method, params) =
            super::super::request(action, Some("creator".into()), Some("nonce".into()))
                .unwrap();
        assert_eq!(method, "blueprint.validate.create");
        assert_eq!(params["create"]["handlers"], normalized);
    }
}

#[test]
fn source_recipes_expose_defaults_and_override_inputs_without_changing_kind() {
    for (name, assignments, expected_kind) in [
        ("github", vec!["repo=owner/project", "id=42", "resource=pr_checks", "interval=20s", "timeout_ms=5000", "done=stop", "result=notify"], "github"),
        ("github-failing", vec!["repo=owner/project", "id=42", "failing=stop"], "github"),
        ("github-runners", vec!["repo=owner/project", "label=linux-build", "offline=stop"], "github"),
        ("github-org-runners", vec!["org=owner", "online=stop"], "github"),
        ("http-health", vec!["url=https://example.com/health", "expected_status=204", "timeout_ms=1000", "down=stop"], "http"),
    ] {
        let assignments = assignments.into_iter().map(str::to_string).collect::<Vec<_>>();
        let spec = bound_pipeline_spec(name, &assignments).unwrap();
        assert_eq!(spec["steps"][0]["kind"]["kind"], expected_kind);
        let parsed: codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec = serde_json::from_value(spec.clone()).unwrap();
        assert!(codeterm_blueprint_types::blueprint_pipeline::validate(&parsed, &[]).is_ok(), "{name}");
        if name == "github" {
            assert_eq!(spec["trigger"]["every"], "20s");
            assert_eq!(spec["steps"][0]["kind"]["probe"]["timeout_ms"], 5000);
            assert_eq!(spec["steps"][0]["on"]["done"], "stop");
            assert!(spec["steps"][0]["on"].get("failing").is_none());
            assert_eq!(spec["result"]["to"], "notify");
        }
    }
}
