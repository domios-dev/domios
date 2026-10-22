use crate::blueprint_recipe::{Recipe, MAX_RECIPE_BYTES, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod positions;
pub mod schema;
#[cfg(test)]
mod tests;
mod validate;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Value>,
}

fn team_owner() -> String {
    "team".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub field: String,
    pub severity: String,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<Value>,
}

pub fn check(file: &str, source: &str, assignments: &[String]) -> Check {
    let mut diagnostics = Vec::new();
    let positions = positions::Positions::parse(if source.len() <= MAX_RECIPE_BYTES {
        source
    } else {
        ""
    });
    for (field, line, column) in &positions.duplicates {
        diagnostics.push(Diagnostic {
            file: file.into(),
            line: *line,
            column: *column,
            field: field.clone(),
            severity: "error".into(),
            message: "Duplicate mapping key".into(),
            hint: "Keep one value per key".into(),
        });
    }
    let mut emit = |field: &str, message: &str, hint: &str, warning: bool| {
        let (line, column) = positions.at(field);
        diagnostics.push(Diagnostic {
            file: file.into(),
            line,
            column,
            field: field.into(),
            severity: if warning { "warning" } else { "error" }.into(),
            message: message.into(),
            hint: hint.into(),
        });
    };
    let value = if source.len() > MAX_RECIPE_BYTES {
        emit(
            "",
            "Blueprint document exceeds 256 KiB",
            "Split this definition into smaller Blueprints",
            false,
        );
        None
    } else {
        match serde_yaml::from_str::<Value>(source) {
            Ok(value) => Some(value),
            Err(error) => {
                let location = error.location();
                diagnostics.push(Diagnostic { file: file.into(), line: location.as_ref().map_or(1, |p| p.line()),
                    column: location.as_ref().map_or(1, |p| p.column()), field: String::new(), severity: "error".into(),
                    message: error.to_string(), hint: "Use a single YAML mapping with unique string keys; tags and includes are unsupported".into() });
                None
            }
        }
    };
    let definition = value.and_then(|value| {
        validate::document(&value, &mut |field, message, hint, warning| {
            let (line,column) = positions.at(field);
            diagnostics.push(Diagnostic {file:file.into(),line,column,field:field.into(),severity:if warning {"warning"} else {"error"}.into(),message:message.into(),hint:hint.into()});
        });
        let document: Document = match serde_json::from_value(value) {
            Ok(document) => document,
            Err(error) => {
                diagnostics.push(Diagnostic{file:file.into(),line:1,column:1,field:String::new(),severity:"error".into(),message:error.to_string(),hint:"Use blueprint schema to inspect the document fields".into()});
                return None;
            },
        };
        match document.bind(assignments) {
            Ok(definition) => {
                validate::definition(&definition, &mut |field, message, hint, warning| {
                    let (line,column) = positions.at(field);
                    diagnostics.push(Diagnostic {file:file.into(),line,column,field:field.into(),severity:if warning {"warning"} else {"error"}.into(),message:message.into(),hint:hint.into()});
                });
                Some(definition)
            }
            Err(errors) => {
                for (field,message) in errors {
                    let (line,column) = positions.at(&field);
                    diagnostics.push(Diagnostic {file:file.into(),line,column,field,severity:"error".into(),message,hint:"Declare a matching typed input and supply its default or --set key=value".into()});
                }
                None
            }
        }
    });
    let ok = definition.is_some() && !diagnostics.iter().any(|d| d.severity == "error");
    Check {
        ok,
        diagnostics,
        definition: if ok { definition } else { None },
    }
}

impl Document {
    pub fn template_errors(&self) -> Vec<(String, String)> {
        let value = match serde_json::to_value(self) {
            Ok(value) => value,
            Err(error) => return vec![(String::new(), error.to_string())],
        };
        let mut errors = Vec::new();
        validate::document(&value, &mut |field, message, _, warning| {
            if !warning {
                errors.push((field.into(), message.into()));
            }
        });
        errors
    }

    pub fn bind(&self, assignments: &[String]) -> Result<Value, Vec<(String, String)>> {
        let mut errors = Vec::new();
        let mut values = self
            .inputs
            .iter()
            .filter_map(|(key, input)| input.default.clone().map(|value| (key.clone(), value)))
            .collect::<BTreeMap<_, _>>();
        let mut seen = std::collections::BTreeSet::new();
        for assignment in assignments {
            let Some((key, raw)) = assignment.split_once('=') else {
                errors.push(("inputs".into(), "Use --set key=value".into()));
                continue;
            };
            let Some(input) = self.inputs.get(key) else {
                errors.push(("inputs".into(), format!("Unknown input {key}")));
                continue;
            };
            if !seen.insert(key) {
                errors.push((format!("inputs.{key}"), "Duplicate input assignment".into()));
            }
            let value = if input.kind == "string" {
                Value::String(raw.into())
            } else {
                match serde_json::from_str(raw) {
                    Ok(value) => value,
                    Err(error) => {
                        errors.push((
                            format!("inputs.{key}"),
                            format!("Invalid JSON input value: {error}"),
                        ));
                        continue;
                    }
                }
            };
            values.insert(key.into(), value);
        }
        for (key, input) in &self.inputs {
            if !crate::blueprint_placeholder::valid_key(key) {
                errors.push((format!("inputs.{key}"), "Use an input key of letters, digits, dots, underscores or hyphens, without a leading dot".into()));
            }
            match values.get(key) {
                Some(value) if input.accepts(value) => {}
                Some(_) => errors.push((
                    format!("inputs.{key}"),
                    format!("Expected {} input", input.kind),
                )),
                None => errors.push((format!("inputs.{key}"), "Missing required input".into())),
            }
        }
        let mut document =
            serde_json::to_value(self).map_err(|error| vec![(String::new(), error.to_string())])?;
        document
            .as_object_mut()
            .map(|fields| fields.remove("inputs"));
        // Names expose identities in lists and notifications; URL credentials stay private.
        let name_values = values
            .iter()
            .map(|(key, value)| {
                let safe = value
                    .as_str()
                    .and_then(|raw| {
                        let parsed = url::Url::parse(raw).ok()?;
                        if !matches!(parsed.scheme(), "http" | "https") {
                            return None;
                        }
                        let host = parsed.host_str()?;
                        let port = parsed
                            .port()
                            .map(|port| format!(":{port}"))
                            .unwrap_or_default();
                        Some(json!(format!("{host}{port}{}", parsed.path())))
                    })
                    .unwrap_or_else(|| value.clone());
                (key.clone(), safe)
            })
            .collect::<BTreeMap<_, _>>();
        let mut safe_name = json!(self.name);
        substitute(&mut safe_name, "name", &name_values, &mut errors);
        substitute(&mut document, "", &values, &mut errors);
        document["name"] = safe_name;
        if !errors.is_empty() {
            return Err(errors);
        }
        let resolved_name = document["name"].as_str().unwrap_or("");
        if crate::blueprint_placeholder::tokens(&self.name)
            .iter()
            .any(|token| token.key.is_some())
            && resolved_name.chars().count() > 40
        {
            let hash = resolved_name
                .bytes()
                .fold(0xcbf29ce484222325u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                });
            let prefix = resolved_name
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || ch == '-' {
                        ch
                    } else {
                        '-'
                    }
                })
                .take(33)
                .collect::<String>();
            let prefix = prefix.trim_matches('-');
            let prefix = if prefix.is_empty() {
                "blueprint"
            } else {
                prefix
            };
            document["name"] = json!(format!("{prefix}-{:06x}", hash & 0xffffff));
        }
        let mut definition =
            if let Some(pipeline) = document.get_mut("pipeline").filter(|p| p.is_object()) {
                let mut spec = pipeline.clone();
                for key in ["name", "note"] {
                    spec[key] = document[key].clone();
                }
                spec["owner"] = document.get("owner").cloned().unwrap_or(json!("team"));
                if let Some(steps) = spec["steps"].as_array_mut() {
                    for step in steps {
                        let kind = if let Some(run) = step.get("run") {
                            json!({"kind":"command","command":run})
                        } else {
                            let uses = step["uses"].as_str().unwrap_or("");
                            let params = step.get("with").cloned().unwrap_or(json!({}));
                            if matches!(uses, "http" | "github") {
                                json!({"kind":uses,"probe":params})
                            } else {
                                let mut kind = params;
                                if let Some(fields) = kind.as_object_mut() {
                                    fields.insert("kind".into(), json!(uses));
                                }
                                kind
                            }
                        };
                        if let Some(fields) = step.as_object_mut() {
                            fields.remove("uses");
                            fields.remove("with");
                            fields.remove("run");
                            fields.insert("kind".into(), kind);
                        }
                    }
                }
                json!({"spec":spec})
            } else if let Some(watch) = document.get("watch").filter(|p| p.is_object()) {
                let mut create = watch.clone();
                for key in ["name", "note"] {
                    create[key] = document[key].clone();
                }
                if let Some(observe) = create
                    .get("observe")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                {
                    let (kind, id) = observe.split_once(':').ok_or_else(|| {
                        vec![(
                            "watch.observe".into(),
                            "Use tab:<id> or cron-run:<id>".into(),
                        )]
                    })?;
                    create["observe"] =
                        json!({"kind":if kind=="cron-run" {"cron_run"} else {kind},"id":id});
                }
                let mut typed: crate::blueprint_watch::Create = serde_json::from_value(create)
                    .map_err(|error| vec![("watch".into(), error.to_string())])?;
                typed.normalize_handlers();
                json!({"create":typed})
            } else {
                return Err(vec![(
                    String::new(),
                    "Choose exactly one watch or pipeline mapping".into(),
                )]);
            };
        if let Some(spec) = definition.get_mut("spec") {
            if let Some(steps) = spec["steps"].as_array_mut() {
                for step in steps {
                    if step.pointer("/kind/kind").and_then(Value::as_str) == Some("http") {
                        if let Some(status) = step.pointer_mut("/kind/probe/expected_status") {
                            if status.is_number() {
                                *status = json!(status.to_string());
                            }
                        }
                    }
                }
            }
            crate::blueprint_recipe::normalize_pipeline_inputs(spec)
                .map_err(|error| vec![("pipeline".into(), error)])?;
        }
        Ok(definition)
    }

    pub fn from_definition(definition: &Value) -> Result<Self, String> {
        let (key, body) = if let Some(spec) = definition.get("spec") {
            ("pipeline", spec)
        } else {
            (
                "watch",
                definition.get("create").ok_or("Expected spec or create")?,
            )
        };
        let mut body = body.clone();
        let fields = body.as_object_mut().ok_or("Definition must be an object")?;
        let name = fields
            .remove("name")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or("Blueprint name required")?;
        let note = fields
            .remove("note")
            .and_then(|v| v.as_str().map(str::to_owned));
        let owner = fields
            .remove("owner")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(team_owner);
        if key == "pipeline" {
            if let Some(steps) = body["steps"].as_array_mut() {
                for step in steps {
                    let Some(fields) = step.as_object_mut() else {
                        continue;
                    };
                    let mut kind = fields.remove("kind").ok_or("Step kind required")?;
                    let uses = kind["kind"]
                        .as_str()
                        .ok_or("Step kind required")?
                        .to_owned();
                    if uses == "command" {
                        fields.insert("run".into(), kind["command"].clone());
                    } else {
                        kind.as_object_mut()
                            .ok_or("Step kind must be an object")?
                            .remove("kind");
                        let params = if matches!(uses.as_str(), "http" | "github") {
                            kind["probe"].clone()
                        } else {
                            kind
                        };
                        fields.insert("uses".into(), json!(uses));
                        fields.insert("with".into(), params);
                    }
                }
            }
        }
        if key == "watch" {
            if let Some(fields) = body.as_object_mut() {
                fields.retain(|_, value| !value.is_null());
            }
        }
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            name,
            note,
            owner: if key == "pipeline" { Some(owner) } else { None },
            inputs: BTreeMap::new(),
            watch: if key == "watch" {
                Some(body.clone())
            } else {
                None
            },
            pipeline: if key == "pipeline" { Some(body) } else { None },
        })
    }

    pub fn from_recipe(recipe: &Recipe) -> Result<Self, String> {
        let mut document = Self::from_definition(&recipe.definition)?;
        document.inputs = recipe
            .placeholders
            .iter()
            .map(|(key, description)| {
                (
                    key.clone(),
                    Input {
                        kind: "string".into(),
                        default: None,
                        description: Some(description.clone()),
                        choices: None,
                    },
                )
            })
            .collect();
        Ok(document)
    }

    pub fn yaml(&self) -> Result<String, String> {
        serde_yaml::to_string(self).map_err(|error| error.to_string())
    }
}

impl Input {
    pub fn accepts(&self, value: &Value) -> bool {
        if self
            .choices
            .as_ref()
            .is_some_and(|choices| !choices.contains(value))
        {
            return false;
        }
        match self.kind.as_str() {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            _ => false,
        }
    }
}

fn substitute(
    value: &mut Value,
    path: &str,
    values: &BTreeMap<String, Value>,
    errors: &mut Vec<(String, String)>,
) {
    match value {
        Value::String(text) => {
            let source = text.clone();
            let mut offset = 0;
            let mut result = String::new();
            for token in crate::blueprint_placeholder::tokens(&source) {
                result.push_str(&source[offset..token.span.start]);
                match token
                    .key
                    .and_then(|key| values.get(key).map(|value| (key, value)))
                {
                    Some((_, replacement)) if token.span == (0..source.len()) => {
                        *value = replacement.clone();
                        return;
                    }
                    Some((_, Value::String(replacement))) => result.push_str(replacement),
                    Some((_, replacement))
                        if replacement.is_number() || replacement.is_boolean() =>
                    {
                        result.push_str(&replacement.to_string())
                    }
                    Some((key, _)) => errors.push((
                        path.into(),
                        format!("Input {key} must occupy the whole value"),
                    )),
                    None => result.push_str(&source[token.span.clone()]),
                }
                offset = token.span.end;
            }
            result.push_str(&source[offset..]);
            *text = result;
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                substitute(item, &format!("{path}[{i}]"), values, errors)
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if !crate::blueprint_placeholder::tokens(key).is_empty() {
                    errors.push((path.into(), "Inputs are allowed only in values".into()));
                }
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                substitute(value, &next, values, errors);
            }
        }
        _ => {}
    }
}

pub fn definition_errors(definition: &Value) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    if definition.as_object().is_none_or(|fields| {
        fields.len() != 1 || (!fields.contains_key("spec") && !fields.contains_key("create"))
    }) {
        return vec![(
            String::new(),
            "Choose exactly one spec or create wire definition".into(),
        )];
    }
    validate::definition(definition, &mut |field, message, _, warning| {
        if !warning {
            errors.push((field.into(), message.into()))
        }
    });
    errors
}

pub fn diagnostic(file: &str, source: &str, field: &str, message: &str, hint: &str) -> Diagnostic {
    let (line, column) = positions::Positions::parse(source).at(field);
    Diagnostic {
        file: file.into(),
        line,
        column,
        field: field.into(),
        message: message.into(),
        hint: hint.into(),
        severity: "error".into(),
    }
}
