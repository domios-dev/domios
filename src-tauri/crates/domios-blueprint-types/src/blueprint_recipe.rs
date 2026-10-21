use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_RECIPE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema_version: u32,
    pub placeholders: BTreeMap<String, String>,
    pub definition: Value,
}

impl Recipe {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_RECIPE_BYTES {
            return Err("Blueprint recipe exceeds 256 KiB".into());
        }
        let recipe: Self = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn validate(&self) -> Result<(), String> {
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_RECIPE_BYTES
        {
            return Err("Blueprint recipe exceeds 256 KiB".into());
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported Blueprint schema version: {}",
                self.schema_version
            ));
        }
        let fields = self
            .definition
            .as_object()
            .ok_or("recipe definition must be an object")?;
        if fields.len() != 1
            || !fields
                .get("create")
                .or_else(|| fields.get("spec"))
                .is_some_and(Value::is_object)
        {
            return Err("recipe definition must contain exactly one create or spec object".into());
        }
        let names = placeholder_names(&self.definition)?;
        if names != self.placeholders.keys().cloned().collect() {
            return Err("placeholder descriptions must match the recipe placeholders".into());
        }
        if self
            .placeholders
            .values()
            .any(|description| description.trim().is_empty() || description.len() > 256)
        {
            return Err("placeholder descriptions must be nonempty and bounded".into());
        }
        Ok(())
    }

    pub fn bind(&self, assignments: &[String]) -> Result<Value, String> {
        self.validate()?;
        let mut value = self.definition.clone();
        substitute(&mut value, assignments)?;
        Ok(value)
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn placeholder_names(value: &Value) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    visit_strings(value, &mut |text| {
        let mut rest = text;
        while let Some((prefix, tail)) = rest.split_once("{{") {
            if prefix.contains("}}") {
                return Err("malformed recipe placeholder".into());
            }
            let (key, tail) = tail
                .split_once("}}")
                .ok_or("malformed recipe placeholder")?;
            if !valid_key(key) {
                return Err("invalid recipe placeholder name".into());
            }
            names.insert(key.into());
            rest = tail;
        }
        if rest.contains("}}") {
            return Err("malformed recipe placeholder".into());
        }
        Ok(())
    })?;
    Ok(names)
}

fn visit_strings(
    value: &Value,
    visitor: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    match value {
        Value::String(text) => visitor(text)?,
        Value::Array(values) => {
            for value in values {
                visit_strings(value, visitor)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key.contains("{{") || key.contains("}}") {
                    return Err("recipe placeholders are supported only in values".into());
                }
                visit_strings(value, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn substitute(value: &mut Value, assignments: &[String]) -> Result<(), String> {
    let names = placeholder_names(value)?;
    let mut replacements = BTreeMap::new();
    for assignment in assignments {
        let (key, replacement) = assignment
            .split_once('=')
            .ok_or("--set must use key=value")?;
        if !valid_key(key)
            || replacement.is_empty()
            || replacement.contains("{{")
            || replacement.contains("}}")
        {
            return Err(
                "--set requires a valid key and a nonempty value without placeholders".into(),
            );
        }
        if !names.contains(key) {
            return Err(format!(
                "--set key `{key}` does not match a recipe placeholder"
            ));
        }
        if replacements
            .insert(key.to_string(), replacement.to_string())
            .is_some()
        {
            return Err(format!("duplicate --set key `{key}`"));
        }
    }
    let missing = names
        .iter()
        .filter(|key| !replacements.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "missing recipe placeholders: {}",
            missing.join(", ")
        ));
    }
    replace(value, &replacements);
    if serde_json::to_vec(value)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_RECIPE_BYTES
    {
        return Err("bound Blueprint recipe exceeds 256 KiB".into());
    }
    Ok(())
}

fn replace(value: &mut Value, replacements: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            let source = std::mem::take(text);
            let mut rest = source.as_str();
            while let Some((prefix, tail)) = rest.split_once("{{") {
                let Some((key, tail)) = tail.split_once("}}") else {
                    break;
                };
                text.push_str(prefix);
                text.push_str(&replacements[key]);
                rest = tail;
            }
            text.push_str(rest);
        }
        Value::Array(values) => {
            for value in values {
                replace(value, replacements);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace(value, replacements);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn blueprint_recipe_refuses_unknown_schema_and_missing_placeholders() {
        let mut recipe = Recipe {
            schema_version: 2,
            placeholders: BTreeMap::from([
                ("tab".into(), "Destination tab".into()),
                ("path".into(), "Probe path".into()),
            ]),
            definition: json!({"create":{"observe":"{{tab}}", "probe":"{{path}}"}}),
        };
        assert!(recipe.bind(&[]).unwrap_err().contains("schema version"));
        recipe.schema_version = SCHEMA_VERSION;
        assert_eq!(
            recipe.bind(&[]).unwrap_err(),
            "missing recipe placeholders: path, tab"
        );
        assert_eq!(
            recipe
                .bind(&["tab=other".into(), "path=/scratch".into()])
                .unwrap(),
            json!({"create":{"observe":"other", "probe":"/scratch"}})
        );
    }

    #[test]
    fn blueprint_recipe_bind_is_simultaneous_and_refuses_extra_fields() {
        let mut value = json!({"probe":"{{a}} {{b}}"});
        substitute(&mut value, &["a=$b".into(), "b=ready".into()]).unwrap();
        assert_eq!(value["probe"], "$b ready");
        assert!(Recipe::parse(
            r#"{"schema_version":1,"placeholders":{},"definition":{},"runtime":{}}"#
        )
        .is_err());
    }
}
