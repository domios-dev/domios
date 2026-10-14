use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const HANDLER_TYPES: &[&str] = &[
    "notify",
    "stop",
    "blueprint",
    "send",
    "cron",
    "respond_prompt",
    "escalate",
    "command",
];

pub(super) fn declared_outputs(
    routes: Vec<String>,
    path: Option<&Path>,
    buckets: &[String],
) -> Result<(BTreeMap<String, String>, Option<Value>), String> {
    if path.is_some() && !routes.is_empty() {
        return Err("--route and --handlers-file cannot be combined".into());
    }
    if let Some(path) = path {
        let raw = read_bounded(path, "handlers file")?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("invalid handlers file JSON: {error}"))?;
        let definitions = validate_handler_file(value, buckets)?;
        return Ok((BTreeMap::new(), Some(definitions)));
    }

    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut single = BTreeMap::new();
    let mut silent = BTreeSet::new();
    let mut repeated = false;
    let mut ids = BTreeSet::new();
    for route in routes {
        let (bucket, action) = route.split_once('=').ok_or("use bucket=action")?;
        if !buckets.iter().any(|declared| declared == bucket) {
            return Err(format!("--route names undeclared bucket `{bucket}`"));
        }
        repeated |= single
            .insert(bucket.to_string(), action.to_string())
            .is_some();
        if super::map_none_route(bucket, action, &mut grouped, &mut silent)? {
            continue;
        }
        let (kind, target) = action
            .split_once(':')
            .map(|(kind, target)| (kind, Some(target)))
            .unwrap_or((action, None));
        let id = unique_id(kind, target, None, bucket, &ids);
        ids.insert(id.clone());
        grouped.entry(bucket.to_string()).or_default().push(json!({
            "id": id,
            "type": kind,
            "parameters": target.map(|target| json!({"target": target})).unwrap_or_else(|| json!({}))
        }));
    }
    if repeated {
        Ok((
            BTreeMap::new(),
            Some(serde_json::to_value(grouped).map_err(|error| error.to_string())?),
        ))
    } else {
        Ok((single, None))
    }
}

pub(super) fn read_bounded(path: &Path, label: &str) -> Result<String, String> {
    use std::io::Read;

    let mut raw = String::new();
    std::fs::File::open(path)
        .map_err(|error| format!("{label}: {error}"))?
        .take(65_537)
        .read_to_string(&mut raw)
        .map_err(|error| format!("{label}: {error}"))?;
    if raw.len() > 65_536 {
        return Err(format!("{label} exceeds 64 KiB"));
    }
    Ok(raw)
}

pub(super) fn validate_handler_file(value: Value, buckets: &[String]) -> Result<Value, String> {
    let mut root = value.as_object().cloned().ok_or_else(|| {
        field_error(
            "",
            "an object of bucket names to handler arrays",
            "bucket names",
            "{}",
        )
    })?;
    let mut ids = BTreeSet::new();
    for (bucket, handlers) in &mut root {
        if bucket != "other" && !buckets.iter().any(|declared| declared == bucket) {
            let allowed = if buckets.is_empty() {
                "other".to_string()
            } else {
                format!("{}, other", buckets.join(", "))
            };
            return Err(field_error(
                &pointer_segment("", bucket),
                "a declared bucket or reserved `other` bucket",
                &allowed,
                &minimal_example(bucket, "notify", "{}"),
            ));
        }
        let array = handlers.as_array_mut().ok_or_else(|| {
            field_error(
                &pointer_segment("", bucket),
                "an array of handler objects",
                "JSON arrays",
                &minimal_example(bucket, "notify", "{}"),
            )
        })?;
        if array.len() > 16 {
            return Err(field_error(
                &pointer_segment("", bucket),
                "at most 16 handlers",
                "0..=16 handlers per bucket",
                &minimal_example(bucket, "notify", "{}"),
            ));
        }
        for (index, handler) in array.iter_mut().enumerate() {
            let path = format!("{}/{index}", pointer_segment("", bucket));
            normalize_handler(handler, &path, bucket, &mut ids)?;
        }
    }
    if ids.len() > 64 {
        return Err(field_error(
            "",
            "at most 64 handler nodes",
            "0..=64 handlers total",
            "{\"passed\":[{\"type\":\"notify\",\"parameters\":{}}]}",
        ));
    }
    Ok(Value::Object(root))
}

fn normalize_handler(
    value: &mut Value,
    path: &str,
    bucket: &str,
    ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    let handler = value.as_object_mut().ok_or_else(|| {
        field_error(
            path,
            "an object with type and parameters",
            "id, type, parameters",
            &minimal_example(bucket, "notify", "{}"),
        )
    })?;
    reject_unknown(
        handler,
        &["id", "type", "parameters"],
        path,
        &minimal_example(bucket, "notify", "{}"),
    )?;
    let kind = handler.get("type").and_then(Value::as_str).ok_or_else(|| {
        field_error(
            &pointer_segment(path, "type"),
            "a handler type string",
            &HANDLER_TYPES.join(", "),
            &minimal_example(bucket, "notify", "{}"),
        )
    })?;
    if !HANDLER_TYPES.contains(&kind) {
        return Err(field_error(
            &pointer_segment(path, "type"),
            "one of the supported handler types",
            &HANDLER_TYPES.join(", "),
            &minimal_example(bucket, "notify", "{}"),
        ));
    }
    let parameters = handler
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            field_error(
                &pointer_segment(path, "parameters"),
                "an object of parameters",
                parameter_keys(kind),
                &minimal_example(bucket, "notify", "{}"),
            )
        })?;
    let allowed = match kind {
        "command" => &["command", "label", "timeout_ms"][..],
        "notify" | "stop" => &[][..],
        _ => &["target"][..],
    };
    reject_unknown(
        parameters,
        allowed,
        &pointer_segment(path, "parameters"),
        &minimal_example(bucket, kind, parameter_example(kind)),
    )?;
    let target = parameters.get("target").and_then(Value::as_str);
    validate_parameters(kind, parameters, path, bucket)?;
    let raw_id = handler.get("id");
    let id = match raw_id {
        Some(value) => {
            let id = value.as_str().ok_or_else(|| {
                field_error(
                    &pointer_segment(path, "id"),
                    "a meaningful handler id string",
                    "1..=64 ASCII letters, numbers, `_` or `-`; omit id to generate one",
                    &minimal_example(
                        bucket,
                        kind,
                        &serde_json::to_string(parameters).unwrap_or_default(),
                    ),
                )
            })?;
            if is_generic_id(id) {
                return Err(field_error(
                    &pointer_segment(path, "id"),
                    "a meaningful handler id",
                    "omit id to generate a type-and-bucket name",
                    &minimal_example(
                        bucket,
                        kind,
                        &serde_json::to_string(parameters).unwrap_or_default(),
                    ),
                ));
            }
            if id.is_empty()
                || id.len() > 64
                || !id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
            {
                return Err(field_error(
                    &pointer_segment(path, "id"),
                    "a bounded handler id using ASCII letters, numbers, `_` or `-`",
                    "1..=64 characters",
                    &minimal_example(
                        bucket,
                        kind,
                        &serde_json::to_string(parameters).unwrap_or_default(),
                    ),
                ));
            }
            id.to_string()
        }
        None => generated_id(
            kind,
            target,
            parameters.get("label").and_then(Value::as_str),
            bucket,
            ids,
        ),
    };
    if !ids.insert(id.clone()) {
        return Err(field_error(
            &pointer_segment(path, "id"),
            "a unique handler id",
            "one id per handler node",
            &minimal_example(
                bucket,
                kind,
                &serde_json::to_string(parameters).unwrap_or_default(),
            ),
        ));
    }
    handler.insert("id".into(), json!(id));
    Ok(())
}

fn validate_parameters(
    kind: &str,
    parameters: &Map<String, Value>,
    path: &str,
    bucket: &str,
) -> Result<(), String> {
    let parameters_path = pointer_segment(path, "parameters");
    let target = parameters.get("target");
    if matches!(kind, "notify" | "stop") {
        return Ok(());
    }
    if kind != "command" {
        let target = target.and_then(Value::as_str).ok_or_else(|| {
            let (allowed, sample) = match kind {
                "respond_prompt" => (
                    "permission, question, or another supported prompt kind",
                    "{\"target\":\"permission\"}",
                ),
                "escalate" => (
                    "creator or agent:<same-Team-tab>",
                    "{\"target\":\"creator\"}",
                ),
                "blueprint" => (
                    "an existing same-scope blueprint id",
                    "{\"target\":\"blueprint_id\"}",
                ),
                "send" => (
                    "an existing same-Team agent tab id",
                    "{\"target\":\"tab_id\"}",
                ),
                _ => (
                    "an existing same-scope cron name or id",
                    "{\"target\":\"cron_name\"}",
                ),
            };
            field_error(
                &pointer_segment(&parameters_path, "target"),
                "a nonempty target string",
                allowed,
                &minimal_example(bucket, kind, sample),
            )
        })?;
        if target.trim().is_empty() || target.len() > 256 || target.chars().any(char::is_control) {
            return Err(field_error(
                &pointer_segment(&parameters_path, "target"),
                "a nonempty bounded target string without control characters",
                "1..=256 characters",
                &minimal_example(bucket, kind, "{\"target\":\"target_id\"}"),
            ));
        }
        return Ok(());
    }
    let command = parameters
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            field_error(
                &pointer_segment(&parameters_path, "command"),
                "nonempty fixed command text",
                "1..=8192 bytes without NUL",
                &minimal_example(
                    bucket,
                    kind,
                    "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
                ),
            )
        })?;
    if command.trim().is_empty() || command.len() > 8192 || command.contains('\0') {
        return Err(field_error(
            &pointer_segment(&parameters_path, "command"),
            "nonempty fixed command text without NUL",
            "1..=8192 bytes",
            &minimal_example(
                bucket,
                kind,
                "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
            ),
        ));
    }
    let label = parameters
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            field_error(
                &pointer_segment(&parameters_path, "label"),
                "a bounded command display label",
                "1..=256 characters without control characters",
                &minimal_example(
                    bucket,
                    kind,
                    "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
                ),
            )
        })?;
    if label.trim().is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
        return Err(field_error(
            &pointer_segment(&parameters_path, "label"),
            "a bounded command display label",
            "1..=256 characters without control characters",
            &minimal_example(
                bucket,
                kind,
                "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
            ),
        ));
    }
    let timeout = parameters
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            field_error(
                &pointer_segment(&parameters_path, "timeout_ms"),
                "a positive command timeout in milliseconds",
                "1..=3000 on Unix or 1..=10000 on Windows",
                &minimal_example(
                    bucket,
                    kind,
                    "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
                ),
            )
        })?;
    #[cfg(windows)]
    let runtime_ms = 10_000;
    #[cfg(not(windows))]
    let runtime_ms = 3_000;
    if timeout == 0 || timeout > runtime_ms {
        return Err(field_error(
            &pointer_segment(&parameters_path, "timeout_ms"),
            "a positive command timeout within the platform runtime",
            &format!("1..={runtime_ms}"),
            &minimal_example(
                bucket,
                kind,
                "{\"command\":\"printf done\",\"label\":\"Report done\",\"timeout_ms\":1000}",
            ),
        ));
    }
    Ok(())
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    path: &str,
    example: &str,
) -> Result<(), String> {
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        let allowed_values = if allowed.is_empty() {
            "none".to_string()
        } else {
            allowed.join(", ")
        };
        return Err(field_error(
            &pointer_segment(path, unknown),
            "a supported field",
            &allowed_values,
            example,
        ));
    }
    Ok(())
}

fn parameter_keys(kind: &str) -> &'static str {
    match kind {
        "notify" | "stop" => "no parameters",
        "command" => "command, label, timeout_ms",
        _ => "target",
    }
}

fn parameter_example(kind: &str) -> &'static str {
    match kind {
        "command" => r#"{"command":"printf done","label":"Report done","timeout_ms":1000}"#,
        "notify" | "stop" => "{}",
        "respond_prompt" => r#"{"target":"permission"}"#,
        "escalate" => r#"{"target":"creator"}"#,
        "blueprint" => r#"{"target":"blueprint_id"}"#,
        "send" => r#"{"target":"target_id"}"#,
        _ => r#"{"target":"cron_name"}"#,
    }
}

fn generated_id(
    kind: &str,
    target: Option<&str>,
    label: Option<&str>,
    bucket: &str,
    ids: &BTreeSet<String>,
) -> String {
    (1..)
        .map(|ordinal| {
            codeterm_blueprint_types::automation_name::generated_handler_id(
                if kind == "blueprint" {
                    "sentinel"
                } else {
                    kind
                },
                target,
                label,
                bucket,
                ordinal,
            )
        })
        .find(|id| !ids.contains(id))
        .expect("generated handler ids are unbounded by ordinal")
}

fn unique_id(
    kind: &str,
    target: Option<&str>,
    label: Option<&str>,
    bucket: &str,
    ids: &BTreeSet<String>,
) -> String {
    let mut ordinal = 1;
    loop {
        let id = codeterm_blueprint_types::automation_name::generated_handler_id(
            if kind == "blueprint" {
                "sentinel"
            } else {
                kind
            },
            target,
            label,
            bucket,
            ordinal,
        );
        if !ids.contains(&id) {
            return id;
        }
        ordinal += 1;
    }
}

fn is_generic_id(id: &str) -> bool {
    let (prefix, suffix) = id.split_once('_').unwrap_or(("", ""));
    matches!(prefix.to_ascii_lowercase().as_str(), "default" | "handler")
        && !suffix.is_empty()
        && suffix.chars().all(|character| character.is_ascii_digit())
}

fn pointer_segment(parent: &str, segment: &str) -> String {
    format!("{parent}/{}", segment.replace('~', "~0").replace('/', "~1"))
}

fn field_error(path: &str, expected: &str, allowed: &str, example: &str) -> String {
    format!(
        "handlers file JSON pointer {path:?}: expected {expected}; allowed values: {allowed}; minimal corrected example: {example}"
    )
}

fn minimal_example(bucket: &str, kind: &str, parameters: &str) -> String {
    format!(
        "{{\"{}\":[{{\"type\":\"{}\",\"parameters\":{}}}]}}",
        bucket, kind, parameters
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<Value, String> {
        let value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        validate_handler_file(value, &["passed".into(), "waiting".into()])
    }

    #[test]
    fn missing_ids_are_generated_from_type_and_bucket() {
        let handlers = parse(r#"{"passed":[{"type":"notify","parameters":{}}]}"#).unwrap();
        assert_eq!(handlers["passed"][0]["id"], "notify-on-passed");
    }

    #[test]
    fn precise_pointer_errors_describe_expected_allowed_values_and_correction() {
        let error = parse(r#"{"passed":[{"type":"notify","parameters":{}},{"type":"send","parameters":{"tab":"tab_1"}}]}"#).unwrap_err();
        assert!(error.contains("/passed/1/parameters/tab"));
        assert!(error.contains("expected a supported field"));
        assert!(error.contains("allowed values: target"));
        assert!(error.contains("\"parameters\":{\"target\":\"target_id\"}"));
    }

    #[test]
    fn refuses_generic_ids_and_unknown_keys_by_name() {
        for id in ["default_1", "handler_2"] {
            let generic = parse(&format!(
                r#"{{"passed":[{{"id":"{id}","type":"notify","parameters":{{}}}}]}}"#
            ))
            .unwrap_err();
            assert!(generic.contains("omit id to generate a type-and-bucket name"));
        }
        let unknown = parse(
            r#"{"passed":[{"id":"review","type":"notify","parameters":{},"unexpected":true}]}"#,
        )
        .unwrap_err();
        assert!(unknown.contains("/passed/0/unexpected"));
        assert!(unknown.contains("unexpected"));
    }
}
