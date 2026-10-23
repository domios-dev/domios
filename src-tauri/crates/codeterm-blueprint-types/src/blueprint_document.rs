use crate::blueprint_recipe::{Recipe, MAX_RECIPE_BYTES, SCHEMA_VERSION};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub mod calls;
pub mod checker;
pub use checker::Diagnostic;
mod positions;
pub mod schema;
mod types;
pub use types::*;
pub mod code_contract;
mod lower;
pub mod projection;
#[cfg(test)]
mod tests;
mod validate;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Document {
    #[serde(deserialize_with = "version")]
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u32,
    pub kind: Kind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, Port>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub imports: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, calls::FrozenCallee>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    pub steps: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<Node>,
    #[serde(default)]
    pub result: ResultPort,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blueprints: BTreeMap<String, SubBlueprint>,
}
fn version<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(d)?;
    if version == SCHEMA_VERSION {
        Ok(version)
    } else {
        Err(serde::de::Error::custom(
            "Only schema_version: 2 is supported",
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parsed {
    pub document: Document,
    pub locations: BTreeMap<String, Location>,
}
impl Parsed {
    pub fn ir(&self) -> projection::PositionedIr {
        projection::PositionedIr {
            canonical: projection::canonical(&self.document),
            locations: self.locations.clone(),
        }
    }

    pub fn at(&self, field: &str) -> Location {
        let mut path = field.to_owned();
        loop {
            if let Some(location) = self.locations.get(&path) {
                return location.clone();
            }
            let Some(end) = path.rfind(['.', '[']) else {
                return Location {
                    file: String::new(),
                    line: 1,
                    column: 1,
                };
            };
            path.truncate(end);
        }
    }
}

pub fn parse(file: &str, source: &str) -> Result<Parsed, Vec<Diagnostic>> {
    if source.len() > MAX_RECIPE_BYTES {
        return Err(vec![diagnostic(
            file,
            "",
            "",
            "Blueprint document exceeds 256 KiB",
            "Split the graph",
        )]);
    }
    let positions = positions::Positions::parse(source);
    if !positions.duplicates.is_empty() {
        return Err(positions
            .duplicates
            .iter()
            .map(|(field, line, column)| {
                Diagnostic::error(
                    Location {
                        file: file.into(),
                        line: *line,
                        column: *column,
                    },
                    field,
                    "Duplicate mapping key",
                    "Keep one value per key",
                )
            })
            .collect());
    }
    let document: Document = serde_yaml::from_str(source).map_err(|error| {
        let loc = error.location();
        vec![Diagnostic::error(
            Location {
                file: file.into(),
                line: loc.as_ref().map_or(1, |l| l.line()),
                column: loc.as_ref().map_or(1, |l| l.column()),
            },
            "",
            &error.to_string(),
            "Use the native v2 schema; values use lit, input or bind",
        )]
    })?;
    let locations = positions
        .locations
        .into_iter()
        .map(|(path, (line, column))| {
            (
                path,
                Location {
                    file: file.into(),
                    line,
                    column,
                },
            )
        })
        .collect();
    Ok(Parsed {
        document,
        locations,
    })
}

pub fn check(file: &str, source: &str, assignments: &[String]) -> Check {
    check_with_context(file, source, assignments, &checker::CheckContext::default())
}

pub fn check_with_context(
    file: &str,
    source: &str,
    assignments: &[String],
    context: &checker::CheckContext,
) -> Check {
    let parsed = match parse(file, source) {
        Ok(parsed) => parsed,
        Err(diagnostics) => {
            return Check {
                ok: false,
                diagnostics,
                definition: None,
            }
        }
    };
    let mut diagnostics = checker::check_ir(&parsed.ir(), context);
    if checker::has_errors(&diagnostics) {
        return Check {
            ok: false,
            diagnostics,
            definition: None,
        };
    }
    let definition = match parsed.document.bind(assignments) {
        Ok(definition) => definition,
        Err(errors) => {
            diagnostics.extend(errors.into_iter().map(|(field, message)| {
                Diagnostic::error(
                    parsed.at(&field),
                    &field,
                    &message,
                    "Use explicit --set key=value bindings",
                )
            }));
            return Check {
                ok: false,
                diagnostics,
                definition: None,
            };
        }
    };
    let bound: Document = match serde_json::from_value(definition.clone()) {
        Ok(doc) => doc,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                parsed.at(""),
                "",
                &error.to_string(),
                "Correct the bound document",
            ));
            return Check {
                ok: false,
                diagnostics,
                definition: None,
            };
        }
    };
    let bound_ir = Parsed {
        document: bound.clone(),
        locations: parsed.locations,
    }
    .ir();
    diagnostics = checker::check_ir(&bound_ir, context);
    if !checker::has_errors(&diagnostics) {
        if let Err(errors) = bound.lower() {
            diagnostics.extend(errors.into_iter().map(|(field, message)| {
                let mut diagnostic = diagnostic(
                    file,
                    source,
                    &field,
                    &message,
                    "Use a construct supported by the runtime",
                );
                diagnostic.rule = "runtime".into();
                diagnostic.name = "runtime_validation".into();
                diagnostic.id = format!("runtime:{}", diagnostic.proof.atom.id);
                diagnostic
            }));
        }
    }
    let ok = !checker::has_errors(&diagnostics);
    Check {
        ok,
        diagnostics,
        definition: ok.then_some(definition),
    }
}

impl Document {
    pub fn template_errors(&self) -> Vec<(String, String)> {
        validate::document(self)
    }
    pub fn from_definition(value: &Value) -> Result<Self, String> {
        let doc: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let errors = doc.template_errors();
        if errors.is_empty() {
            Ok(doc)
        } else {
            Err(errors
                .into_iter()
                .map(|(p, m)| format!("{p}: {m}"))
                .collect::<Vec<_>>()
                .join("; "))
        }
    }
    pub fn from_recipe(recipe: &Recipe) -> Result<Self, String> {
        recipe.validate()?;
        Self::from_definition(&recipe.definition)
    }
    pub fn yaml(&self) -> Result<String, String> {
        let errors = self.template_errors();
        if !errors.is_empty() {
            return Err(format!("Invalid v2 document: {errors:?}"));
        }
        serde_yaml::to_string(self).map_err(|e| e.to_string())
    }
    pub fn bind(&self, assignments: &[String]) -> Result<Value, Vec<(String, String)>> {
        let mut errors = self.template_errors();
        let mut values = self
            .inputs
            .iter()
            .filter_map(|(key, p)| p.default.clone().map(|v| (key.clone(), v)))
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
            let value = if input.kind == "string"
                || input.accepts_with_types(&Value::String(raw.into()), &self.types)
            {
                Value::String(raw.into())
            } else {
                match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push((format!("inputs.{key}"), e.to_string()));
                        continue;
                    }
                }
            };
            values.insert(key.into(), value);
        }
        for (key, input) in &self.inputs {
            match values.get(key) {
                Some(v) if input.accepts_with_types(v, &self.types) => {}
                Some(_) => errors.push((
                    format!("inputs.{key}"),
                    format!("Value violates input schema {}", input.kind),
                )),
                None => errors.push((format!("inputs.{key}"), "Missing required input".into())),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut bound = self.clone();
        if let Some(key) = whole_input(&bound.name) {
            bound.name = values
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| vec![("name".into(), "Name requires a string input".into())])?
                .into();
        }
        for (index, node) in bound.steps.iter_mut().chain(&mut bound.other).enumerate() {
            for (key, form) in node.params.iter_mut().chain(&mut node.matches) {
                resolve_form(
                    form,
                    &values,
                    &format!("steps[{index}].with.{key}"),
                    &mut errors,
                );
            }
            for form in node
                .run
                .iter_mut()
                .chain(&mut node.every)
                .chain(&mut node.timeout)
            {
                resolve_form(form, &values, &format!("steps[{index}]"), &mut errors);
            }
            if let Some(call) = &mut node.call {
                for (key, form) in &mut call.arguments {
                    resolve_form(
                        form,
                        &values,
                        &format!("steps[{index}].call.with.{key}"),
                        &mut errors,
                    );
                }
            }
        }
        if let Some(trigger) = &mut bound.trigger {
            match trigger {
                Trigger::Schedule { every } => {
                    resolve_form(every, &values, "trigger.every", &mut errors)
                }
                Trigger::Event { kind, params } => {
                    resolve_form(kind, &values, "trigger.kind", &mut errors);
                    for (key, form) in params {
                        resolve_form(form, &values, &format!("trigger.params.{key}"), &mut errors);
                    }
                }
                Trigger::Input { kind, source } => {
                    resolve_form(kind, &values, "trigger.kind", &mut errors);
                    resolve_form(source, &values, "trigger.source", &mut errors);
                }
            }
        }
        if let Some(ResultTarget::Input { input }) = &bound.result.to {
            let target = values.get(input).and_then(Value::as_str).ok_or_else(|| {
                vec![(
                    "result.to".into(),
                    "Result target requires a string author input".into(),
                )]
            })?;
            bound.result.to = Some(ResultTarget::Fixed(target.into()));
        }
        bound.inputs.clear();
        errors.extend(bound.template_errors());
        if errors.is_empty() {
            serde_json::to_value(bound).map_err(|e| vec![(String::new(), e.to_string())])
        } else {
            Err(errors)
        }
    }
    pub fn lower(&self) -> Result<Value, Vec<(String, String)>> {
        lower::lower(self)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.steps.iter().chain(&self.other)
    }
}
fn resolve_form(
    form: &mut ValueForm,
    values: &BTreeMap<String, Value>,
    path: &str,
    errors: &mut Vec<(String, String)>,
) {
    if let ValueForm::Input { input } = form {
        match values.get(input) {
            Some(value) => *form = ValueForm::literal(value.clone()),
            None => errors.push((path.into(), format!("Undeclared input {input}"))),
        }
    }
}
pub(super) fn whole_input(text: &str) -> Option<&str> {
    text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}"))
}
pub fn definition_errors(value: &Value) -> Vec<(String, String)> {
    let document: Document = match serde_json::from_value(value.clone()) {
        Ok(document) => document,
        Err(error) => return vec![(String::new(), error.to_string())],
    };
    let diagnostics = checker::check_document(&document, &checker::CheckContext::default());
    if checker::has_errors(&diagnostics) {
        diagnostics
            .into_iter()
            .filter(|d| d.severity == "error")
            .map(|d| (d.field.clone(), checker::render(&[d], true)))
            .collect()
    } else {
        document.lower().err().unwrap_or_default()
    }
}
pub fn diagnostic(file: &str, source: &str, field: &str, message: &str, hint: &str) -> Diagnostic {
    let (line, column) = positions::Positions::parse(source).at(field);
    Diagnostic::error(
        Location {
            file: file.into(),
            line,
            column,
        },
        field,
        message,
        hint,
    )
}
