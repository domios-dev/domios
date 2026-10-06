use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_SPEC_BYTES: usize = 256 * 1024;

pub(super) fn load(
    spec_path: Option<&Path>,
    recipe: Option<&str>,
    assignments: &[String],
    owner: Option<&str>,
) -> Result<Value, String> {
    let mut value = match (spec_path, recipe) {
        (Some(path), None) => {
            if !assignments.is_empty() {
                return Err("--set requires --recipe".into());
            }
            parse_spec(&read_spec(path)?)?
        }
        (None, Some(name)) => {
            let mut value = recipe_spec(name)?;
            substitute(&mut value, assignments)?;
            value
        }
        _ => return Err("choose exactly one of --spec or --recipe".into()),
    };
    if let Some(owner) = owner {
        value
            .as_object_mut()
            .ok_or("pipeline specification must be an object")?
            .insert("owner".into(), Value::String(owner.into()));
    }
    if !value.is_object() {
        return Err("pipeline specification must be an object".into());
    }
    Ok(value)
}

fn read_spec(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin()
            .take((MAX_SPEC_BYTES + 1) as u64)
            .read_to_string(&mut input)
            .map_err(|error| format!("read pipeline specification: {error}"))?;
        if input.len() > MAX_SPEC_BYTES {
            return Err("pipeline specification exceeds 256 KiB".into());
        }
        return Ok(input);
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("read pipeline specification: {error}"))?;
    if bytes.len() > MAX_SPEC_BYTES {
        return Err("pipeline specification exceeds 256 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "pipeline specification must be UTF-8".into())
}

fn parse_spec(raw: &str) -> Result<Value, String> {
    match serde_json::from_str(raw) {
        Ok(value) => Ok(value),
        Err(json_error) => serde_yaml::from_str(raw).map_err(|yaml_error| {
            format!("invalid pipeline specification: JSON ({json_error}); YAML ({yaml_error})")
        }),
    }
}

fn recipe_spec(name: &str) -> Result<Value, String> {
    let mut remaining = include_str!("../../../../prod-docs/BLUEPRINT-RECIPES.md");
    while let Some((_, tail)) = remaining.split_once("<!-- blueprint-pipeline-recipe:") {
        let (header, body) = tail
            .split_once("-->")
            .ok_or("pipeline recipe marker is incomplete")?;
        let (content, rest) = body
            .split_once("<!-- /blueprint-pipeline-recipe -->")
            .ok_or_else(|| format!("pipeline recipe `{}` is incomplete", header.trim()))?;
        if header.trim() == name {
            let spec = fenced(content, "yaml")
                .ok_or_else(|| format!("pipeline recipe `{name}` needs a YAML specification"))?;
            return parse_spec(spec);
        }
        remaining = rest;
    }
    Err(format!("unknown pipeline recipe `{name}`"))
}

fn fenced(body: &str, language: &str) -> Option<&str> {
    let opening = format!("```{language}");
    let (_, tail) = body.split_once(&opening)?;
    let (content, _) = tail.split_once("```")?;
    Some(content.trim())
}

fn substitute(value: &mut Value, assignments: &[String]) -> Result<(), String> {
    let mut replacements = BTreeMap::new();
    for assignment in assignments {
        let (key, value) = assignment
            .split_once('=')
            .ok_or("--set must use key=value")?;
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err("--set key must be nonempty, bounded and printable".into());
        }
        if replacements
            .insert(key.to_string(), value.to_string())
            .is_some()
        {
            return Err(format!("duplicate --set key `{key}`"));
        }
    }
    let mut used = BTreeSet::new();
    replace_strings(value, &replacements, &mut used)?;
    if let Some(unused) = replacements.keys().find(|key| !used.contains(*key)) {
        return Err(format!(
            "--set key `{unused}` does not match a recipe placeholder"
        ));
    }
    Ok(())
}

fn replace_strings(
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
) -> Result<(), String> {
    match value {
        Value::String(text) => {
            for (key, replacement) in replacements {
                let placeholder = format!("{{{{{key}}}}}");
                if text.contains(&placeholder) {
                    *text = text.replace(&placeholder, replacement);
                    used.insert(key.clone());
                }
            }
            if text.contains("{{") || text.contains("}}") {
                return Err(format!("recipe has an unset placeholder in `{text}`"));
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_strings(value, replacements, used)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace_strings(value, replacements, used)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_spec_reader_parses_json_and_yaml_and_applies_owner() {
        let dir = tempfile::tempdir().unwrap();
        for (name, source) in [
            (
                "spec.json",
                r#"{"name":"echo","trigger":{"type":"schedule","every":"20s"},"steps":[]}"#,
            ),
            (
                "spec.yaml",
                "name: echo\ntrigger:\n  type: schedule\n  every: 20s\nsteps: []\n",
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, source).unwrap();
            let value = load(Some(&path), None, &[], Some("team")).unwrap();
            assert_eq!(value["name"], "echo");
            assert_eq!(value["owner"], "team");
        }
    }

    #[test]
    fn blueprint_recipe_set_replaces_declared_placeholders_once() {
        let mut value = serde_json::json!({"command": "printf '{{message}}'"});
        substitute(&mut value, &["message=ready".into()]).unwrap();
        assert_eq!(value["command"], "printf 'ready'");
        assert!(substitute(&mut value, &["missing=x".into()]).is_err());
    }
}
