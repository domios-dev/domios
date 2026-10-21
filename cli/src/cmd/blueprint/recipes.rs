use serde::Serialize;

const SOURCE: &str = include_str!("../../../../prod-docs/BLUEPRINT-RECIPES.md");
#[derive(Clone, Debug, Serialize)]
pub(super) struct Recipe {
    pub name: String,
    pub summary: String,
    pub handlers: String,
    pub command: String,
    pub defaults: std::collections::BTreeMap<String, String>,
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
        let defaults = if pipeline {
            fenced(body, "json").map(serde_json::from_str).transpose().map_err(|error| format!("recipe `{name}` defaults: {error}"))?.unwrap_or_default()
        } else { std::collections::BTreeMap::new() };
        recipes.push(Recipe {
            name: name.into(),
            summary: summary.into(),
            handlers,
            command: command.into(),
            defaults,
            spec,
        });
        remaining = rest;
    }
    Ok(recipes)
}

#[cfg(test)]
pub(super) fn pipeline_spec(name: &str) -> Result<serde_json::Value, String> {
    recipes()?
        .into_iter()
        .find(|recipe| recipe.name == name)
        .and_then(|recipe| recipe.spec)
        .ok_or_else(|| format!("unknown pipeline recipe `{name}`"))
}

pub(super) fn bound_pipeline_spec(name: &str, assignments: &[String]) -> Result<serde_json::Value, String> {
    let recipe = recipes()?.into_iter().find(|recipe| recipe.name == name).ok_or_else(|| format!("unknown pipeline recipe `{name}`"))?;
    let mut value = recipe.spec.ok_or_else(|| format!("recipe `{name}` is not a pipeline"))?;
    let mut merged = recipe.defaults.iter().filter(|(key, _)| !assignments.iter().any(|assignment| assignment.split_once('=').is_some_and(|(input, _)| input == key.as_str()))).map(|(key,value)| format!("{key}={value}")).collect::<Vec<_>>();
    merged.extend_from_slice(assignments);
    codeterm_blueprint_types::blueprint_recipe::substitute(&mut value, &merged)?;
    codeterm_blueprint_types::blueprint_recipe::normalize_pipeline_inputs(&mut value)?;
    if let Some(steps) = value.get_mut("steps").and_then(serde_json::Value::as_array_mut) {
        for step in steps {
            if step.pointer("/kind/probe/resource").and_then(serde_json::Value::as_str) == Some("pr_checks") {
                if let Some(on) = step.get_mut("on").and_then(serde_json::Value::as_object_mut) { on.remove("failing"); }
            }
        }
    }
    Ok(value)
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
                println!("Defaults (override with --set): {}", serde_json::to_string(&recipe.defaults).unwrap_or_default());
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
        println!("\nPrint a ready-to-edit specification or handlers file and --dry-run command with `codeterm blueprint recipe <name>`.");
    }
}

#[cfg(test)]
mod tests;
