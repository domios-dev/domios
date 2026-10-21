use crate::blueprint_pipeline::{self, Step, Trigger};
use serde_json::Value;
type Emit<'a> = dyn FnMut(&str, &str, &str, bool) + 'a;

pub(super) fn document(value: &Value, emit: &mut Emit<'_>) {
    lint_inputs(value, "", value["inputs"].as_object(), emit);
    let schema = super::schema::schema();
    shape(value, &schema, &schema, "", emit);
    if value.get("watch").is_some() == value.get("pipeline").is_some() {
        emit(
            "",
            "Choose exactly one watch or pipeline",
            "Remove one section or add watch/pipeline",
            false,
        );
    }
    if value["note"].as_str().is_none_or(|s| s.trim().is_empty()) {
        emit(
            "note",
            "A note helps explain why this Blueprint exists",
            "Add a short note",
            true,
        );
    }
    if let Some(inputs) = value["inputs"].as_object() {
        for (name, input) in inputs {
            if let Ok(input) = serde_json::from_value::<super::Input>(input.clone()) {
                if input
                    .default
                    .as_ref()
                    .is_some_and(|value| !input.accepts(value))
                {
                    emit(
                        &format!("inputs.{name}.default"),
                        "Default does not match the declared input type",
                        "Use a value of the declared type",
                        false,
                    );
                }
            }
        }
    }
    if value.get("watch").is_some() && value.get("owner").is_some() {
        emit(
            "owner",
            "Watch ownership is fixed to its creator in the caller Team",
            "Remove owner; owner is a pipeline option",
            false,
        )
    }
    if let Some(steps) = value.pointer("/pipeline/steps").and_then(Value::as_array) {
        for (i, step) in steps.iter().enumerate() {
            if step["uses"] == "github" && step.pointer("/with/id").is_some_and(Value::is_number) {
                emit(
                    &format!("pipeline.steps[{i}].with.id"),
                    "Hardcoded run or PR ID limits reuse",
                    "Declare an integer input and use '{{id}}'",
                    true,
                );
            }
        }
    }
}

fn shape(value: &Value, schema: &Value, root: &Value, path: &str, emit: &mut Emit<'_>) {
    if value.as_str().is_some_and(|s| {
        crate::blueprint_placeholder::tokens(s)
            .iter()
            .any(|token| token.key.is_some())
    }) {
        return;
    }
    if let Some(reference) = schema["$ref"].as_str() {
        if let Some(schema) = root.pointer(reference.trim_start_matches('#')) {
            shape(value, schema, root, path, emit)
        }
        return;
    }
    if path.starts_with("pipeline.steps[") && !path.contains("].") {
        if let Some(uses) = value["uses"].as_str() {
            if !crate::blueprint_catalog::STEPS
                .iter()
                .any(|node| node.name == uses && uses != "command")
            {
                emit(
                    &format!("{path}.uses"),
                    &format!("Unknown node {uses}"),
                    "Choose http, github, ask, filter, convert, agent; use run for commands",
                    false,
                );
                return;
            }
        }
    }
    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        if schema.get("type").is_none() {
            let selected = if value.get("run").is_some() {
                variants.first()
            } else {
                None
            }
            .or_else(|| {
                variants.iter().find(|variant| {
                    let Some(properties) = variant["properties"].as_object() else {
                        return variant["type"]
                            .as_str()
                            .is_some_and(|kind| type_matches(value, kind));
                    };
                    properties
                        .iter()
                        .filter(|(_, rule)| rule.get("const").is_some())
                        .all(|(key, rule)| value.get(key) == rule.get("const"))
                        && variant["required"].as_array().is_none_or(|keys| {
                            keys.iter()
                                .all(|key| value.get(key.as_str().unwrap_or("")).is_some())
                        })
                })
            })
            .or_else(|| {
                variants.iter().find(|variant| {
                    variant["properties"].as_object().is_some_and(|properties| {
                        properties.iter().any(|(key, rule)| {
                            rule.get("const").is_some() && value.get(key) == rule.get("const")
                        })
                    })
                })
            });
            if let Some(selected) = selected {
                shape(value, selected, root, path, emit)
            } else {
                emit(
                    path,
                    "Unknown node kind, missing fields, or incompatible value",
                    "Use a known uses node with with parameters, or run with command text",
                    false,
                );
            }
            return;
        }
    }
    if let Some(kind) = schema["type"].as_str() {
        if !type_matches(value, kind) {
            emit(
                path,
                &format!("Expected {kind}"),
                "Use the declared field type",
                false,
            );
            return;
        }
    }
    if schema
        .get("const")
        .is_some_and(|expected| expected != value)
        || schema["enum"]
            .as_array()
            .is_some_and(|values| !values.contains(value))
    {
        emit(
            path,
            &format!("Unsupported value {value}"),
            &format!(
                "Choose {}",
                schema
                    .get("enum")
                    .or_else(|| schema.get("const"))
                    .unwrap_or(&Value::Null)
            ),
            false,
        );
    }
    if let Some(fields) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !fields.contains_key(key) {
                    emit(
                        &join(path, key),
                        "Missing required field",
                        &format!("Add {key}"),
                        false,
                    );
                }
            }
        }
        for (key, value) in fields {
            let field = join(path, key);
            if let Some(rule) = schema["properties"].get(key) {
                shape(value, rule, root, &field, emit)
            } else if schema["additionalProperties"] == false {
                emit(
                    &field,
                    "Unknown field",
                    "Remove this field; use blueprint schema to inspect supported fields",
                    false,
                )
            } else if schema["additionalProperties"].is_object() {
                shape(value, &schema["additionalProperties"], root, &field, emit)
            }
        }
    }
    if let Some(items) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|min| items.len() < (min as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|max| items.len() > (max as usize))
        {
            emit(
                path,
                "Array length is outside the supported bounds",
                "Adjust the number of items",
                false,
            );
        }
        for (i, item) in items.iter().enumerate() {
            if schema.get("items").is_some() {
                shape(item, &schema["items"], root, &format!("{path}[{i}]"), emit)
            }
        }
    }
    if let Some(text) = value.as_str() {
        if schema["minLength"]
            .as_u64()
            .is_some_and(|min| text.chars().count() < (min as usize))
            || schema["maxLength"]
                .as_u64()
                .is_some_and(|max| text.chars().count() > (max as usize))
        {
            emit(
                path,
                "Text length is outside the supported bounds",
                "Use nonempty bounded text",
                false,
            );
        }
    }
    if let Some(number) = value.as_f64() {
        if schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max)
        {
            emit(
                path,
                "Number is outside the supported bounds",
                "Use a value within the schema minimum and maximum",
                false,
            );
        }
    }
}
fn type_matches(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}
fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.into()
    } else {
        format!("{path}.{key}")
    }
}

pub(super) fn definition(value: &Value, emit: &mut Emit<'_>) {
    if let Some(spec) = value.get("spec") {
        let trigger = decode::<Trigger>(&spec["trigger"], "pipeline.trigger", emit);
        let steps = spec["steps"]
            .as_array()
            .map(|steps| {
                steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        decode::<Step>(step, &format!("pipeline.steps[{index}]"), emit)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let result = spec
            .get("result")
            .map(|result| {
                decode::<blueprint_pipeline::BlueprintResult>(result, "pipeline.result", emit)
            })
            .unwrap_or(Some(Default::default()));
        if let Err(errors) = blueprint_pipeline::validate_parts(
            spec["name"].as_str().unwrap_or(""),
            spec["note"].as_str(),
            trigger.as_ref(),
            &steps,
            result.as_ref(),
            None,
        ) {
            for error in errors {
                emit(
                    &error.path.document_field(),
                    &error.reason,
                    "Check node input types, duration units and forward route names",
                    false,
                );
            }
        }
        for (index, step) in steps.iter().enumerate() {
            if step.as_ref().is_some_and(|step| {
                matches!(step.kind, blueprint_pipeline::StepKind::Convert { .. })
            }) {
                emit(
                    &format!("pipeline.steps[{index}].with"),
                    "Converter availability is checked at apply",
                    "Ensure the required converter plugin is installed",
                    true,
                );
            }
        }
        if matches!(trigger, Some(Trigger::Event { .. })) {
            emit(
                "pipeline.trigger",
                "Event source availability is checked at apply",
                "Install the event source plugin before applying",
                true,
            );
        }
        if steps.iter().all(Option::is_some) {
            unreachable(&steps.into_iter().flatten().collect::<Vec<_>>(), emit);
        }
    }
    if let Some(watch) = value.get("create") {
        watch_checks(watch, emit)
    }
}
fn decode<T: serde::de::DeserializeOwned>(
    value: &Value,
    path: &str,
    emit: &mut Emit<'_>,
) -> Option<T> {
    match serde_json::from_value(value.clone()) {
        Ok(value) => Some(value),
        Err(error) => {
            emit(
                path,
                &error.to_string(),
                "Supply required fields with their declared types",
                false,
            );
            None
        }
    }
}
fn unreachable(steps: &[Step], emit: &mut Emit<'_>) {
    let mut reached = std::collections::BTreeSet::from([0]);
    for (i, step) in steps.iter().enumerate() {
        if !reached.contains(&i) {
            emit(
                &format!("pipeline.steps[{i}].name"),
                "Unreachable stage",
                "Route an earlier outcome here or remove this stage",
                false,
            );
            continue;
        }
        let outcomes = step.kind.outcomes().unwrap_or_default();
        for outcome in outcomes {
            let target = blueprint_pipeline::route_target(&step.on, &outcome);
            if target == "next" {
                reached.insert(i + 1);
            } else if let Some(index) = steps.iter().position(|s| s.name == target) {
                reached.insert(index);
            }
        }
    }
}
fn watch_checks(watch: &Value, emit: &mut Emit<'_>) {
    match serde_json::from_value::<crate::blueprint_watch::Create>(watch.clone()) {
        Ok(create) => {
            for (field, message) in create.admission_errors() {
                let field = if matches!(field.as_str(), "name" | "note") {
                    field
                } else {
                    format!("watch.{field}")
                };
                emit(
                    &field,
                    &message,
                    "Use a declared bucket, bounded duration and valid handler parameters",
                    false,
                )
            }
        }
        Err(error) => emit(
            "watch",
            &error.to_string(),
            "Use blueprint schema to inspect watch fields",
            false,
        ),
    }
}

fn lint_inputs(
    value: &Value,
    path: &str,
    inputs: Option<&serde_json::Map<String, Value>>,
    emit: &mut Emit<'_>,
) {
    match value {
        Value::String(text) => {
            for token in crate::blueprint_placeholder::tokens(text) {
                if let Some(key) = token
                    .key
                    .filter(|key| inputs.is_none_or(|inputs| !inputs.contains_key(*key)))
                {
                    emit(
                        path,
                        &format!("Undeclared input {key}; kept as literal text"),
                        "Declare this input or keep the literal command template",
                        true,
                    );
                }
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if key != "inputs" {
                    lint_inputs(value, &join(path, key), inputs, emit)
                }
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                lint_inputs(value, &format!("{path}[{index}]"), inputs, emit)
            }
        }
        _ => {}
    }
}
