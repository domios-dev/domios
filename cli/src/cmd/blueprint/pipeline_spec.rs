#[cfg(test)]
use codeterm_blueprint_types::blueprint_recipe::substitute;
use serde_json::Value;
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
        (None, Some(name)) => super::recipes::bound_pipeline_spec(name, assignments)?,
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

pub(super) fn read_spec(path: &Path) -> Result<String, String> {
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

#[cfg(test)]
mod tests;
