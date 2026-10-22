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
        if let Some(spec) = value.get_mut("spec") {
            normalize_pipeline_inputs(spec)?;
        }
        Ok(value)
    }
}

pub fn normalize_pipeline_inputs(value: &mut Value) -> Result<(), String> {
    let Some(steps) = value.get_mut("steps").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for step in steps {
        let kind = step
            .pointer("/kind/kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if matches!(kind.as_str(), "github" | "http") {
            for key in ["id", "timeout_ms"] {
                if let Some(value) = step.pointer_mut(&format!("/kind/probe/{key}")) {
                    if let Some(text) = value.as_str() {
                        *value = Value::from(text.parse::<u64>().map_err(|_| {
                            format!(
                                "{} {key} must be a positive integer",
                                if kind == "github" { "GitHub" } else { "HTTP" }
                            )
                        })?);
                    }
                }
            }
        }
    }
    Ok(())
}

use crate::blueprint_placeholder::{tokens, valid_key};

pub fn placeholder_names(value: &Value) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    visit_strings(value, &mut |text| {
        for token in tokens(text) {
            if !token.closed {
                return Err("malformed recipe placeholder".into());
            }
            if let Some(key) = token.key {
                names.insert(key.into());
            }
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
                if !tokens(key).is_empty() {
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
        if !valid_key(key) || replacement.is_empty() || !tokens(replacement).is_empty() {
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
            let mut offset = 0;
            for token in tokens(&source) {
                text.push_str(&source[offset..token.span.start]);
                if let Some(replacement) = token.key.and_then(|key| replacements.get(key)) {
                    text.push_str(replacement);
                } else {
                    text.push_str(&source[token.span.clone()]);
                }
                offset = token.span.end;
            }
            text.push_str(&source[offset..]);
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
mod tests;
