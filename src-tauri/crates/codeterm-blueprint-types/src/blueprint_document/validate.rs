use crate::blueprint_pipeline::{self, BlueprintSpec, Step, Trigger};
use serde_json::{json, Value};
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
    if value.as_str().is_some_and(|s| s.contains("{{")) {
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
        let mut repaired = spec.clone();
        if let Err(error) = serde_json::from_value::<Trigger>(spec["trigger"].clone()) {
            emit(
                "pipeline.trigger",
                &error.to_string(),
                "Use schedule, event or input with its required fields",
                false,
            );
            repaired["trigger"] = json!({"type":"schedule","every":"1m"});
        }
        if let Some(steps) = repaired["steps"].as_array_mut() {
            for (i, step) in steps.iter_mut().enumerate() {
                if let Err(error) = serde_json::from_value::<Step>(step.clone()) {
                    emit(
                        &format!("pipeline.steps[{i}]"),
                        &error.to_string(),
                        "Supply all node parameters with their declared types",
                        false,
                    );
                    *step = json!({"name":step["name"].as_str().unwrap_or("invalid"),"input":"text","kind":{"kind":"command","command":"invalid"},"on":step.get("on").cloned().filter(Value::is_object).unwrap_or(json!({}))});
                }
            }
        }
        match serde_json::from_value::<BlueprintSpec>(repaired) {
            Ok(spec) => {
                let media = [
                    crate::sentinel_input::MediaKind::Text,
                    crate::sentinel_input::MediaKind::Audio,
                    crate::sentinel_input::MediaKind::Video,
                    crate::sentinel_input::MediaKind::Image,
                ];
                let converters = media
                    .iter()
                    .flat_map(|from| {
                        media
                            .iter()
                            .map(move |to| blueprint_pipeline::ConverterEdge {
                                from: *from,
                                to: *to,
                            })
                    })
                    .collect::<Vec<_>>();
                if let Err(errors) = blueprint_pipeline::validate(&spec, &converters) {
                    for error in errors {
                        let field = wire_field(&error.field);
                        emit(
                            &field,
                            &error.reason,
                            "Check node input types, duration units and forward route names",
                            false,
                        );
                    }
                }
                for (i, step) in spec.steps.iter().enumerate() {
                    if matches!(step.kind, blueprint_pipeline::StepKind::Convert { .. }) {
                        emit(
                            &format!("pipeline.steps[{i}].with"),
                            "Converter availability is checked at apply",
                            "Ensure the required converter plugin is installed",
                            true,
                        )
                    }
                }
                if matches!(spec.trigger, Trigger::Event { .. }) {
                    emit(
                        "pipeline.trigger",
                        "Event source availability is checked at apply",
                        "Install the event source plugin before applying",
                        true,
                    )
                }
                unreachable(&spec, emit);
            }
            Err(error) => emit(
                "pipeline",
                &error.to_string(),
                "Use blueprint schema to inspect required pipeline fields",
                false,
            ),
        }
    }
    if let Some(watch) = value.get("create") {
        watch_checks(watch, emit)
    }
}
fn wire_field(field: &str) -> String {
    if matches!(field, "name" | "note" | "owner") {
        return field.into();
    }
    let mut field = format!("pipeline.{field}");
    for (old, new) in [
        (".http", ".with"),
        (".github", ".with"),
        (".command", ".run"),
        (".question", ".with.question"),
        (".buckets", ".with.buckets"),
        (".pattern", ".with.pattern"),
        (".bucket", ".with.bucket"),
        (".from", ".with.from"),
        (".convert", ".with"),
        (".agent", ".with"),
    ] {
        if field.ends_with(old) {
            field.truncate(field.len() - old.len());
            field.push_str(new);
            break;
        }
    }
    field
}
fn unreachable(spec: &BlueprintSpec, emit: &mut Emit<'_>) {
    let mut reached = std::collections::BTreeSet::from([0]);
    for (i, step) in spec.steps.iter().enumerate() {
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
            let target = step.on.get(&outcome).map(String::as_str).unwrap_or("next");
            if target == "next" {
                reached.insert(i + 1);
            } else if let Some(index) = spec.steps.iter().position(|s| s.name == target) {
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
            let mut rest = text.as_str();
            while let Some((_, tail)) = rest.split_once("{{") {
                let Some((key, tail)) = tail.split_once("}}") else {
                    break;
                };
                let key = key.trim();
                if !key.is_empty()
                    && key
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
                    && inputs.is_none_or(|inputs| !inputs.contains_key(key))
                {
                    emit(
                        path,
                        &format!("Undeclared input {key}; kept as literal text"),
                        "Declare this input or keep the literal command template",
                        true,
                    )
                }
                rest = tail;
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
