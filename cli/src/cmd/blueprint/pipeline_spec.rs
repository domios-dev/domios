use codeterm_types::blueprint_recipe::substitute;
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

fn recipe_spec(name: &str) -> Result<Value, String> {
    super::recipes::pipeline_spec(name)
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
