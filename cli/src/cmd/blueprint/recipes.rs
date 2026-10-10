use serde::Serialize;

const SOURCE: &str = include_str!("../../../../prod-docs/BLUEPRINT-RECIPES.md");
#[derive(Clone, Debug, Serialize)]
pub(super) struct Recipe {
    pub name: String,
    pub summary: String,
    pub handlers: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<serde_json::Value>,
}

pub(super) fn recipes() -> Result<Vec<Recipe>, String> {
    parse_catalog(SOURCE)
}

fn parse_catalog(source: &str) -> Result<Vec<Recipe>, String> {
    let mut recipes = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut remaining = source;
    while let Some((_, tail)) = remaining.split_once("<!-- blueprint-") {
        let (pipeline, tail) = if let Some(tail) = tail.strip_prefix("pipeline-recipe:") {
            (true, tail)
        } else if let Some(tail) = tail.strip_prefix("recipe:") {
            (false, tail)
        } else {
            remaining = tail;
            continue;
        };
        let (header, after_header) = tail
            .split_once("-->")
            .ok_or("recipe block needs a closing marker")?;
        let name = header.trim();
        let closing = if pipeline {
            "<!-- /blueprint-pipeline-recipe -->"
        } else {
            "<!-- /blueprint-recipe -->"
        };
        let (body, rest) = after_header
            .split_once(closing)
            .ok_or_else(|| format!("recipe `{name}` needs an end marker"))?;
        let summary = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("**Purpose:** "))
            .ok_or_else(|| format!("recipe `{name}` needs a purpose"))?;
        let handlers = if pipeline {
            String::new()
        } else {
            fenced(body, "json")
                .ok_or_else(|| format!("recipe `{name}` needs handlers JSON"))?
                .into()
        };
        let spec = if pipeline {
            Some(
                serde_yaml::from_str(
                    fenced(body, "yaml")
                        .ok_or_else(|| format!("recipe `{name}` needs a YAML specification"))?,
                )
                .map_err(|e| format!("recipe `{name}`: {e}"))?,
            )
        } else {
            None
        };
        let command = fenced(body, "bash")
            .ok_or_else(|| format!("recipe `{name}` needs a create command"))?;
        if name.is_empty() || !names.insert(name.to_string()) || !command.contains("--dry-run") {
            return Err(format!(
                "recipe `{name}` has a duplicate name or incomplete dry-run skeleton"
            ));
        }
        recipes.push(Recipe {
            name: name.into(),
            summary: summary.into(),
            handlers,
            command: command.into(),
            spec,
        });
        remaining = rest;
    }
    Ok(recipes)
}

pub(super) fn pipeline_spec(name: &str) -> Result<serde_json::Value, String> {
    recipes()?
        .into_iter()
        .find(|recipe| recipe.name == name)
        .and_then(|recipe| recipe.spec)
        .ok_or_else(|| format!("unknown pipeline recipe `{name}`"))
}

fn fenced<'a>(body: &'a str, language: &str) -> Option<&'a str> {
    let opening = format!("```{language}");
    let (_, tail) = body.split_once(&opening)?;
    let (content, _) = tail.split_once("```")?;
    Some(content.trim())
}

pub(super) fn print(json_mode: bool, name: Option<&str>) {
    let recipes = recipes().unwrap_or_else(|error| crate::cmd::print::fail(1, error));
    if let Some(name) = name {
        let recipe = recipes
            .iter()
            .find(|recipe| recipe.name == name)
            .unwrap_or_else(|| {
                let names = recipes
                    .iter()
                    .map(|recipe| recipe.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                crate::cmd::print::fail(
                    2,
                    format!("unknown Blueprint recipe `{name}`; choose: {names}"),
                )
            });
        if json_mode {
            crate::cmd::print::print_json(&serde_json::to_value(recipe).unwrap_or_default());
        } else {
            if let Some(spec) = &recipe.spec {
                println!(
                    "Recipe: {}\n{}\n\nSpecification:\n{}\n\nDry-run command:\n{}",
                    recipe.name,
                    recipe.summary,
                    serde_yaml::to_string(spec).unwrap_or_default(),
                    recipe.command
                );
                return;
            }
            println!("Recipe: {}\n{}\n\nHandlers JSON (save as handlers.json):\n{}\n\nDry-run command:\n{}", recipe.name, recipe.summary, recipe.handlers, recipe.command);
        }
        return;
    }
    if json_mode {
        crate::cmd::print::print_json(&serde_json::to_value(recipes).unwrap_or_default());
    } else {
        for recipe in recipes {
            println!("{} â€” {}", recipe.name, recipe.summary);
        }
        println!("\nPrint a ready handlers file and --dry-run command with `codeterm blueprint recipe <name>`.");
    }
}

#[cfg(test)]
mod tests {
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
            codeterm_types::automation_name::validate_sentinel_name(name.as_deref().unwrap_or(""))
                .unwrap_or_else(|error| panic!("recipe {}: {error}", recipe.name));
            let (method, params) =
                super::super::request(action, Some("creator".into()), Some("nonce".into()))
                    .unwrap();
            assert_eq!(method, "blueprint.validate.create");
            assert_eq!(params["create"]["handlers"], normalized);
        }
    }
}
