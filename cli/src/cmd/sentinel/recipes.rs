use serde::Serialize;

const SOURCE: &str = include_str!("../../../../prod-docs/SENTINELS.md");
const REQUIRED: &[&str] = &[
    "team-watch",
    "wait-for-build",
    "retry-until-pass",
    "prompt-responder",
    "cron-run-watch",
    "launch-prompt-gate",
    "all-finished",
];

#[derive(Clone, Debug, Serialize)]
pub(super) struct Recipe {
    pub name: String,
    pub summary: String,
    pub handlers: String,
    pub command: String,
}

pub(super) fn recipes() -> Result<Vec<Recipe>, String> {
    let mut recipes = Vec::new();
    let mut remaining = SOURCE;
    while let Some((_, tail)) = remaining.split_once("<!-- sentinel-recipe:") {
        let (header, after_header) = tail
            .split_once("-->")
            .ok_or("recipe block needs a closing marker")?;
        let name = header.trim();
        let (body, rest) = after_header
            .split_once("<!-- /sentinel-recipe -->")
            .ok_or_else(|| format!("recipe `{name}` needs an end marker"))?;
        let summary = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("**Purpose:** "))
            .ok_or_else(|| format!("recipe `{name}` needs a purpose"))?;
        let handlers =
            fenced(body, "json").ok_or_else(|| format!("recipe `{name}` needs handlers JSON"))?;
        let command = fenced(body, "bash")
            .ok_or_else(|| format!("recipe `{name}` needs a create command"))?;
        if name.is_empty() || command.is_empty() || !command.contains("--dry-run") {
            return Err(format!(
                "recipe `{name}` has an incomplete dry-run skeleton"
            ));
        }
        recipes.push(Recipe {
            name: name.into(),
            summary: summary.into(),
            handlers: handlers.into(),
            command: command.into(),
        });
        remaining = rest;
    }
    for required in REQUIRED {
        if recipes
            .iter()
            .filter(|recipe| recipe.name == *required)
            .count()
            != 1
        {
            return Err(format!(
                "recipe catalog must contain `{required}` exactly once"
            ));
        }
    }
    if recipes.len() != REQUIRED.len() {
        return Err("recipe catalog contains an unexpected recipe".into());
    }
    Ok(recipes)
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
                    format!("unknown Sentinel recipe `{name}`; choose: {names}"),
                )
            });
        if json_mode {
            crate::cmd::print::print_json(&serde_json::to_value(recipe).unwrap_or_default());
        } else {
            println!("Recipe: {}\n{}\n\nHandlers JSON (save as handlers.json):\n{}\n\nDry-run command:\n{}", recipe.name, recipe.summary, recipe.handlers, recipe.command);
        }
        return;
    }
    if json_mode {
        crate::cmd::print::print_json(&serde_json::to_value(recipes).unwrap_or_default());
    } else {
        for recipe in recipes {
            println!("{} — {}", recipe.name, recipe.summary);
        }
        println!("\nPrint a ready handlers file and --dry-run command with `codeterm sentinel recipe <name>`.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn every_recipe_contains_handlers_and_a_dry_run_create_command() {
        use clap::Parser;

        for recipe in recipes().unwrap() {
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
            assert_eq!(command.get(0).map(String::as_str), Some("codeterm"));
            assert_eq!(command.get(1).map(String::as_str), Some("sentinel"));
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
            let crate::args::cli::Commands::Sentinel { action } = cli.command else {
                panic!("recipe command did not parse as sentinel create")
            };
            let crate::args::sentinel::SentinelAction::Create { name, dry_run, .. } = &action
            else {
                panic!("recipe command did not parse as sentinel create")
            };
            assert!(*dry_run, "recipe {} omitted --dry-run", recipe.name);
            codeterm_types::automation_name::validate_sentinel_name(name)
                .unwrap_or_else(|error| panic!("recipe {}: {error}", recipe.name));
            let (method, params) =
                super::super::request(action, Some("creator".into()), Some("nonce".into()))
                    .unwrap();
            assert_eq!(method, "sentinel.validate.create");
            assert_eq!(params["create"]["handlers"], normalized);
        }
    }
}
