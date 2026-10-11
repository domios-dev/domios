use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::args::blueprint::{BlueprintAction, BlueprintEscalationAction};
use crate::client::SocketClient;
use crate::cmd::print::{fail, print_json};
mod handlers;
mod pipeline_spec;
mod recipes;

pub fn handle_recipe(json_mode: bool, name: Option<&str>) {
    recipes::print(json_mode, name);
}

fn assignments(values: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut pairs = BTreeMap::new();
    for value in values {
        let (key, value) = value.split_once('=').ok_or("use bucket=value")?;
        if pairs.insert(key.into(), value.into()).is_some() {
            return Err(format!("duplicate assignment for bucket {key}"));
        }
    }
    Ok(pairs)
}

fn read_text_input_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot read text input {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let limit = codeterm_types::blueprint_terminal::MAX_PAYLOAD_BYTES;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("text input must be a regular file of at most 32768 bytes".into());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("text input exceeds 32768 bytes".into());
    }
    String::from_utf8(bytes).map_err(|_| "text input is not UTF-8".into())
}

fn render(job: &Value) {
    if let Some(pipeline) = job.get("pipeline") {
        let current_step = job["current_step"]
            .as_u64()
            .map(|index| format!(" current_step={index}"))
            .unwrap_or_default();
        println!(
            "{} [pipeline] trigger={}{}",
            job["id"].as_str().unwrap_or("?"),
            pipeline["trigger"]["type"].as_str().unwrap_or("?"),
            current_step
        );
        if let Some(steps) = job["steps"].as_array() {
            for step in steps {
                println!(
                    "  {} {} {}{}",
                    step["index"].as_u64().unwrap_or_default(),
                    step["name"].as_str().unwrap_or("?"),
                    step["state"].as_str().unwrap_or("waiting"),
                    step["result_summary"]
                        .as_str()
                        .map(|summary| format!(" result={summary}"))
                        .unwrap_or_default()
                );
            }
        }
        return;
    }
    println!("{}", watch_summary(job));
}

fn watch_summary(job: &Value) -> String {
    format!(
        "{} [{}] bucket={} ×{} classification={} every={}ms last_probe_at={}{}",
        job["id"].as_str().unwrap_or("?"),
        if job
            .pointer("/machine/stopped")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "stopped"
        } else {
            if job
                .pointer("/machine/paused")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "paused"
            } else {
                "watching"
            }
        },
        job["probe_state"]
            .as_str()
            .or_else(|| job.pointer("/machine/last_bucket").and_then(Value::as_str))
            .unwrap_or("unclassified"),
        job["runs_since_transition"].as_u64().unwrap_or(0),
        if job["matches"]
            .as_object()
            .is_some_and(|patterns| !patterns.is_empty())
        {
            "pattern"
        } else if job["model_classification"].as_bool().unwrap_or(false) {
            "model"
        } else {
            "unmatched"
        },
        job["interval_ms"],
        job["last_probe_at"],
        job["last_reason"]
            .as_str()
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default()
    )
}

fn render_history(history: &Value) {
    println!(
        "{} history: current version {}{}",
        history["blueprint_id"]
            .as_str()
            .or_else(|| history["sentinel_id"].as_str())
            .unwrap_or("?"),
        history["current_version"],
        if history["history_truncated"].as_bool().unwrap_or(false) {
            " (older versions truncated)"
        } else {
            ""
        }
    );
    if let Some(versions) = history["versions"].as_array() {
        for version in versions {
            println!(
                "Version {}: {}",
                version["version"],
                serde_json::to_string_pretty(&version["definition"]).unwrap_or_default()
            );
        }
    }
}

fn deleted_message(result: &Value) -> String {
    let deleted: Vec<&str> = result["deleted"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if deleted.is_empty() {
        "No blueprints deleted.".into()
    } else {
        format!("Deleted {}.", deleted.join(", "))
    }
}

fn escalation_resolved_message(transition_id: Option<&str>, result: &Value) -> String {
    format!(
        "Resolved escalation {} for {}.",
        transition_id.unwrap_or("?"),
        result["id"].as_str().unwrap_or("?")
    )
}

fn render_nodes(nodes: &Value) {
    let Some(nodes) = nodes.as_array() else {
        return;
    };
    for role in [
        "trigger", "source", "agent", "logic", "action", "mode", "step",
    ] {
        for node in nodes
            .iter()
            .filter(|node| node["role"].as_str().unwrap_or("step") == role)
        {
            println!(
                "{role} {}: {}",
                node["type"].as_str().unwrap_or("?"),
                node["title"].as_str().unwrap_or("?")
            );
            if let Some(schema) = node.get("params_schema") {
                println!("  params: {schema}");
            }
        }
    }
}

fn render_log(entries: &Value) {
    println!("{}", log_text(entries));
}

fn log_text(entries: &Value) -> String {
    entries
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            format!(
                "{} {} {} {} outcome={} result={} reason={}",
                entry["started_at_ms"].as_u64().unwrap_or_default(),
                entry["step_index"].as_i64().unwrap_or_default(),
                entry["step_name"].as_str().unwrap_or("?"),
                entry["state"].as_str().unwrap_or("?"),
                entry["outcome"].as_str().unwrap_or("-"),
                entry["result_summary"].as_str().unwrap_or("-"),
                entry["reason"].as_str().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pipeline_preview_summary(value: &Value) -> String {
    let spec = value.get("normalized_spec").unwrap_or(value);
    let mut lines = vec![format!(
        "Blueprint: {}",
        spec["name"].as_str().unwrap_or("?")
    )];
    let trigger = &spec["trigger"];
    match trigger["type"].as_str() {
        Some("schedule") => lines.push(format!(
            "Trigger: schedule every {}",
            trigger["every"].as_str().unwrap_or("?")
        )),
        Some("input") => lines.push(format!(
            "Trigger: input kind={} source={}",
            trigger["kind"].as_str().unwrap_or("?"),
            input_ref_summary(&trigger["source"])
        )),
        Some("event") => lines.push(format!(
            "Trigger: event source={}",
            trigger["source"].as_str().unwrap_or("?")
        )),
        _ => lines.push("Trigger: ?".into()),
    }
    if let Some(steps) = spec["steps"].as_array() {
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "Step {}: {} ({}) input={}",
                index + 1,
                step["name"].as_str().unwrap_or("?"),
                step["kind"]["kind"].as_str().unwrap_or("?"),
                step["input"].as_str().unwrap_or("?")
            ));
        }
    }
    lines.push(format!(
        "Result: {}",
        spec.pointer("/result/to")
            .and_then(Value::as_str)
            .unwrap_or("owner")
    ));
    if let Some(checks) = value["checks"].as_array() {
        lines.push("Checks:".into());
        for check in checks {
            lines.push(format!(
                "  {} {}{}",
                if check["passed"].as_bool().unwrap_or(false) {
                    "PASS"
                } else {
                    "FAIL"
                },
                check["field"].as_str().unwrap_or("validation"),
                check["reason"]
                    .as_str()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ));
        }
    }
    lines.join("\n")
}

fn input_ref_summary(value: &Value) -> String {
    if let Some(path) = value.get("file").and_then(Value::as_str) {
        return format!(
            "file:{}",
            std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
    }
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        let host = url
            .split_once("://")
            .and_then(|(_, rest)| {
                rest.split(|character| matches!(character, '/' | '?' | '#'))
                    .next()
            })
            .unwrap_or("?")
            .rsplit('@')
            .next()
            .unwrap_or("?");
        return host.to_string();
    }
    if value.get("text").is_some() {
        return "text".into();
    }
    "?".into()
}

pub fn handle_blueprint(client: &mut SocketClient, json_mode: bool, action: BlueprintAction) {
    let export_out = match &action {
        BlueprintAction::Export { out, .. } => out.clone(),
        _ => None,
    };
    let (method, params) = request(
        action,
        std::env::var("CODETERM_TAB_ID").ok(),
        std::env::var("CODETERM_SESSION_BINDING_NONCE").ok(),
    )
    .unwrap_or_else(|error| fail(2, error));
    let transition_id = params["transition_id"].as_str().map(str::to_owned);
    let result = client
        .call(method, Some(params))
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if method == "blueprint.export" {
        let raw = serde_json::to_string_pretty(&result)
            .unwrap_or_else(|error| fail(1, error.to_string()));
        if let Some(path) = export_out {
            std::fs::write(&path, format!("{raw}\n"))
                .unwrap_or_else(|error| fail(1, error.to_string()));
        } else {
            println!("{raw}");
        }
        return;
    }
    if json_mode {
        print_json(&result);
        return;
    }
    if method == "blueprint.import" {
        let job = result.get("blueprint").unwrap_or(&result);
        render(job);
        println!(
            "Imported {}.",
            if job["machine"]["stopped"].as_bool().unwrap_or(true) {
                "stopped"
            } else {
                "running"
            }
        );
        return;
    }
    if method == "blueprint.pipeline.nodes" {
        render_nodes(&result);
        return;
    }
    if method == "blueprint.log" {
        render_log(&result);
        return;
    }
    if method == "blueprint.pipeline.create" {
        println!(
            "{} owner={} chain={}",
            result["id"].as_str().unwrap_or("?"),
            result
                .pointer("/owner/kind")
                .and_then(Value::as_str)
                .unwrap_or("tab"),
            result["chain"].as_str().unwrap_or("?")
        );
        return;
    }
    if method.starts_with("blueprint.pipeline.validate") {
        println!("{}", pipeline_preview_summary(&result));
        return;
    }
    if method.starts_with("blueprint.validate.") {
        println!("{}", preview_summary(&result));
        return;
    }
    if method == "blueprint.history" {
        render_history(&result);
        return;
    }
    if method == "blueprint.delete" {
        println!("{}", deleted_message(&result));
        return;
    }
    if method == "blueprint.input" {
        println!(
            "Accepted text input for {}.",
            result["id"].as_str().unwrap_or("?")
        );
        return;
    }
    if method == "blueprint.update" || method == "blueprint.pipeline.update" {
        println!(
            "Updated {} to definition version {}.",
            result["id"].as_str().unwrap_or("?"),
            result["definition_version"]
        );
        return;
    }
    if method == "blueprint.escalation.resolve" {
        println!(
            "{}",
            escalation_resolved_message(transition_id.as_deref(), &result)
        );
        return;
    }
    if let Some(jobs) = result.as_array() {
        if jobs.is_empty() {
            println!("No blueprints.");
        }
        for job in jobs {
            render(job);
        }
    } else {
        render(&result);
        if method == "blueprint.show" {
            let source = input_source_line(&result)
                .or_else(|| pipeline_input_source_line(&result))
                .or_else(|| {
                    result
                        .pointer("/observe/kind")
                        .and_then(Value::as_str)
                        .zip(result.pointer("/observe/id").and_then(Value::as_str))
                        .map(|(kind, id)| format!("Observe: {kind}:{id}"))
                })
                .or_else(|| {
                    result["probe"]
                        .as_str()
                        .map(|probe| format!("Probe: {probe}"))
                })
                .unwrap_or_else(|| "Source: ?".into());
            println!(
                "{source}\nQuestion: {}\nCreator: {}",
                result["question"].as_str().unwrap_or("?"),
                result["creator_tab_id"].as_str().unwrap_or("?")
            );
        }
    }
}

fn request(
    action: BlueprintAction,
    caller: Option<String>,
    nonce: Option<String>,
) -> Result<(&'static str, Value), String> {
    let mut params = json!({ "caller_tab_id": caller, "session_binding_nonce": nonce });
    let method = match action {
        BlueprintAction::Export { id, .. } => {
            params["id"] = json!(id);
            "blueprint.export"
        }
        BlueprintAction::Import {
            file,
            set,
            stopped: _,
            start,
        } => {
            let raw = pipeline_spec::read_spec(&file)?;
            let recipe = codeterm_types::blueprint_recipe::Recipe::parse(&raw)?;
            recipe.bind(&set)?;
            params["recipe"] = serde_json::to_value(recipe).map_err(|error| error.to_string())?;
            params["set"] = json!(set);
            params["stopped"] = json!(!start);
            "blueprint.import"
        }
        BlueprintAction::Recipe { .. } => return Err("blueprint recipe is handled locally".into()),
        BlueprintAction::Create {
            spec,
            recipe,
            set,
            owner,
            name,
            probe,
            observe,
            terminal_pattern,
            output_settled,
            session,
            input,
            input_file,
            input_url,
            input_text,
            convert_to,
            convert_timeout,
            question,
            buckets,
            every,
            routes,
            rearm_on_change,
            handlers_file,
            escalation_timeout_ms,
            paused,
            max_transitions,
            max_actions,
            until,
            timeout,
            matches,
            model_classification,
            dry_run,
        } => {
            if spec.is_some() || recipe.is_some() {
                if name.is_some()
                    || terminal_pattern.is_some()
                    || output_settled.is_some()
                    || session
                    || model_classification
                    || probe.is_some()
                    || observe.is_some()
                    || input.is_some()
                    || input_file.is_some()
                    || input_url.is_some()
                    || input_text.is_some()
                    || question.is_some()
                    || buckets.is_some()
                    || every.is_some()
                    || !routes.is_empty()
                    || !rearm_on_change.is_empty()
                    || handlers_file.is_some()
                    || escalation_timeout_ms.is_some()
                    || paused
                    || max_transitions.is_some()
                    || max_actions.is_some()
                    || until.is_some()
                    || timeout.is_some()
                    || !matches.is_empty()
                {
                    return Err("pipeline and legacy stage create options cannot be mixed".into());
                }
                if let Some(path) = recipe
                    .as_deref()
                    .filter(|name| std::path::Path::new(name).is_file())
                {
                    if owner.is_some() {
                        return Err("portable recipes use their declared owner".into());
                    }
                    let raw = pipeline_spec::read_spec(std::path::Path::new(path))?;
                    let recipe = codeterm_types::blueprint_recipe::Recipe::parse(&raw)?;
                    let bound = recipe.bind(&set)?;
                    if dry_run {
                        if let Some(create) = bound.get("create") {
                            params["create"] = create.clone();
                            return Ok(("blueprint.validate.create", params));
                        }
                        params["spec"] = bound
                            .get("spec")
                            .cloned()
                            .ok_or("recipe definition must contain create or spec")?;
                        return Ok(("blueprint.pipeline.validate", params));
                    }
                    params["recipe"] =
                        serde_json::to_value(recipe).map_err(|error| error.to_string())?;
                    params["set"] = json!(set);
                    params["stopped"] = json!(true);
                    return Ok(("blueprint.import", params));
                }
                params["spec"] = pipeline_spec::load(
                    spec.as_deref(),
                    recipe.as_deref(),
                    &set,
                    owner.as_deref(),
                )?;
                return Ok((
                    if dry_run {
                        "blueprint.pipeline.validate"
                    } else {
                        "blueprint.pipeline.create"
                    },
                    params,
                ));
            }
            if !set.is_empty() || owner.is_some() {
                return Err("--set and --owner require --spec or --recipe".into());
            }
            let (routes, handlers) = handlers::declared_outputs(
                routes,
                handlers_file.as_deref(),
                buckets.as_deref().ok_or("--buckets is required")?,
            )?;
            let observe = observe
                .map(|source| observation_binding(&source))
                .transpose()?;
            let input = input_payload(
                input,
                input_file,
                input_url,
                input_text,
                convert_to,
                convert_timeout,
            )?;
            let terminal = (terminal_pattern.is_some() || output_settled.is_some())
                .then(|| json!({"pattern": terminal_pattern, "idle_ms": output_settled}));
            params["create"] = json!({ "name": name.ok_or("--name is required")?, "probe": probe, "observe": observe, "input": input, "question": question.ok_or("--question is required")?, "buckets": buckets.ok_or("--buckets is required")?, "every": every.unwrap_or_else(|| "20s".into()), "routes": routes, "handlers": handlers, "rearm_on_change": rearm_on_change, "escalation_timeout_ms": escalation_timeout_ms, "paused": paused, "max_transitions": max_transitions, "max_actions": max_actions, "matches": assignments(matches)?, "until": until, "timeout": timeout });
            params["create"]["terminal"] = json!(terminal);
            params["create"]["session"] = json!(session);
            params["create"]["model_classification"] = json!(model_classification);
            if dry_run {
                "blueprint.validate.create"
            } else {
                "blueprint.create"
            }
        }
        BlueprintAction::List => "blueprint.list",
        BlueprintAction::Input {
            id,
            from_pad,
            text_file,
        } => {
            let text = match (from_pad, text_file) {
                (Some(path), None) => crate::cmd::payload_source::read_from_pad(&path)?,
                (None, Some(path)) => read_text_input_file(&path)?,
                _ => return Err("provide --from-pad or --text-file".into()),
            };
            if text.is_empty()
                || text.contains('\0')
                || text.len() > codeterm_types::blueprint_terminal::MAX_PAYLOAD_BYTES
            {
                return Err("blueprint text must contain 1..32768 UTF-8 bytes".into());
            }
            params["id"] = json!(id);
            params["text"] = json!(text);
            "blueprint.input"
        }
        BlueprintAction::Nodes => "blueprint.pipeline.nodes",
        BlueprintAction::Show { id } => {
            params["id"] = json!(id);
            "blueprint.show"
        }
        BlueprintAction::Stop { id } => {
            params["id"] = json!(id);
            "blueprint.stop"
        }
        BlueprintAction::Delete { id, stopped, stop } => {
            params["id"] = json!(id);
            params["stopped"] = json!(stopped);
            params["stop"] = json!(stop);
            "blueprint.delete"
        }
        BlueprintAction::Update {
            id,
            expect_version,
            from_file,
            cause,
            dry_run,
        } => {
            params["id"] = json!(id);
            params["expect_version"] = json!(expect_version);
            let update = read_update(&from_file, cause.as_deref())?;
            if let Some(spec) = pipeline_update_spec(&update)? {
                params["spec"] = spec;
                if let Some(cause) = cause {
                    params["cause"] = json!(cause);
                }
                if dry_run {
                    "blueprint.pipeline.validate.update"
                } else {
                    "blueprint.pipeline.update"
                }
            } else {
                params["update"] = update;
                if dry_run {
                    "blueprint.validate.update"
                } else {
                    "blueprint.update"
                }
            }
        }
        BlueprintAction::History { id } => {
            params["id"] = json!(id);
            "blueprint.history"
        }
        BlueprintAction::Log { id, limit } => {
            params["id"] = json!(id);
            params["limit"] = json!(limit);
            "blueprint.log"
        }
        BlueprintAction::Escalation { action } => match action {
            BlueprintEscalationAction::Resolve { id, transition_id } => {
                params["id"] = json!(id);
                params["transition_id"] = json!(transition_id);
                "blueprint.escalation.resolve"
            }
        },
    };
    Ok((method, params))
}

fn observation_binding(source: &str) -> Result<Value, String> {
    let (kind, target) = if let Some(tab_id) = source.strip_prefix("tab:") {
        ("tab", tab_id)
    } else if let Some(cron_id) = source.strip_prefix("cron-run:") {
        ("cron_run", cron_id)
    } else {
        return Err("use tab:<id> or cron-run:<id>".into());
    };
    if target.trim().is_empty() || target.len() > 256 || target.chars().any(char::is_control) {
        return Err("--observe target must be nonempty and bounded".into());
    }
    Ok(json!({"kind":kind, "id":target}))
}

/// `Input: audio file quick-brown-fox.wav → text`; inline text is never rendered.
fn input_source_line(value: &Value) -> Option<String> {
    use codeterm_types::sentinel_input::{display_name, DataRef, InputSpec};
    let spec: InputSpec = serde_json::from_value(value.get("input")?.clone()).ok()?;
    let ref_type = match &spec.data {
        DataRef::Text(_) => "text",
        DataRef::File(_) => "file",
        DataRef::Url(_) => "url",
    };
    let mut line = format!("Input: {} {ref_type}", spec.kind.as_str());
    let name = display_name(&spec.data);
    if !name.is_empty() {
        line.push(' ');
        line.push_str(&name);
    }
    if let Some(convert_to) = spec.convert_to {
        line.push_str(&format!(" → {}", convert_to.as_str()));
    }
    Some(line)
}

fn pipeline_input_source_line(value: &Value) -> Option<String> {
    let trigger = value.pointer("/pipeline/trigger")?;
    if trigger.get("type")?.as_str()? != "input" {
        return None;
    }
    Some(format!(
        "Input: {} source={} convert_to={}",
        trigger.get("kind")?.as_str()?,
        input_ref_summary(trigger.get("source")?),
        trigger
            .get("convert_to")
            .and_then(Value::as_str)
            .unwrap_or("-")
    ))
}

fn absolute_path(path: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    if path.is_absolute() {
        return Ok(path);
    }
    let cwd = std::env::current_dir().map_err(|error| format!("caller cwd: {error}"))?;
    Ok(cwd.join(path))
}

#[allow(clippy::too_many_arguments)]
fn input_payload(
    kind: Option<String>,
    file: Option<std::path::PathBuf>,
    url: Option<String>,
    text: Option<String>,
    convert_to: Option<String>,
    convert_timeout: Option<u64>,
) -> Result<Value, String> {
    use codeterm_types::sentinel_input::{DataRef, InputSpec, MediaKind};
    let Some(kind) = kind else {
        if file.is_some()
            || url.is_some()
            || text.is_some()
            || convert_to.is_some()
            || convert_timeout.is_some()
        {
            return Err("input data and conversion flags require --input".into());
        }
        return Ok(Value::Null);
    };
    let kind = MediaKind::parse(&kind).ok_or("--input: use text, audio, video or image")?;
    let data = match (file, url, text) {
        (Some(file), None, None) => DataRef::File(absolute_path(file)?),
        (None, Some(url), None) => DataRef::Url(url),
        (None, None, Some(text)) => DataRef::Text(text),
        _ => {
            return Err(
                "--input requires exactly one of --input-file, --input-url, --input-text".into(),
            )
        }
    };
    let convert_to = match convert_to.as_deref() {
        Some(target) => Some(MediaKind::parse(target).ok_or("--convert-to: use text or audio")?),
        None => None,
    };
    let spec = InputSpec {
        kind,
        data,
        convert_to,
        convert_timeout_ms: convert_timeout,
    };
    codeterm_types::sentinel_input::validate(&spec).map_err(|error| format!("--input: {error}"))?;
    serde_json::to_value(&spec).map_err(|error| error.to_string())
}

fn read_update(path: &std::path::Path, cause: Option<&str>) -> Result<Value, String> {
    let raw = handlers::read_bounded(path, "definition file")?;
    let mut update: Value =
        serde_json::from_str(&raw).map_err(|error| format!("invalid definition file: {error}"))?;
    let object = update
        .as_object_mut()
        .ok_or("definition file must contain a JSON object")?;
    normalize_update_handlers(object)?;
    if let Some(cause) = cause {
        object.insert("cause".into(), json!(cause));
    }
    Ok(update)
}

fn pipeline_update_spec(update: &Value) -> Result<Option<Value>, String> {
    if update.get("trigger").is_some() {
        return Ok(Some(update.clone()));
    }
    let history = update.get("versions");
    let Some(versions) = history.and_then(Value::as_array) else {
        return Ok(update.get("pipeline").cloned());
    };
    let current = update
        .get("current_version")
        .and_then(Value::as_u64)
        .ok_or("history file needs current_version")?;
    let version = versions
        .iter()
        .find(|version| version.get("version").and_then(Value::as_u64) == Some(current))
        .ok_or("history file has no current definition version")?;
    Ok(version
        .pointer("/definition/pipeline")
        .filter(|pipeline| !pipeline.is_null())
        .cloned())
}

fn preview_summary(job: &Value) -> String {
    let mut lines = vec![format!(
        "Blueprint graph: {}",
        job["name"].as_str().unwrap_or("(unnamed)")
    )];
    lines.push(format!(
        "Buckets: {}",
        job["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines.push("Handlers:".into());
    if let Some(routes) = job["routes"].as_object() {
        for (bucket, handlers) in routes {
            lines.push(format!("  {bucket}:"));
            if let Some(handlers) = handlers.as_array() {
                for handler in handlers {
                    let action =
                        serde_json::to_string(&handler["action"]).unwrap_or_else(|_| "?".into());
                    lines.push(format!(
                        "    {} -> {action}",
                        handler["id"].as_str().unwrap_or("?")
                    ));
                }
            }
        }
    }
    lines.push(format!("Cadence: {} ms", job["interval_ms"]));
    lines.push(format!(
        "Budgets: {} transitions, {} actions",
        job["max_transitions"], job["max_actions"]
    ));
    lines.join("\n")
}

fn normalize_update_handlers(object: &mut serde_json::Map<String, Value>) -> Result<(), String> {
    let has_handlers = object.contains_key("handlers");
    let has_other = object.contains_key("on_other");
    if !has_handlers && !has_other {
        return Ok(());
    }
    let buckets = object
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or("definition JSON pointer `/buckets`: expected an array of bucket names")?
        .iter()
        .map(|bucket| {
            bucket.as_str().map(str::to_string).ok_or_else(|| {
                "definition JSON pointer `/buckets`: expected each bucket to be a string".into()
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut declared = object
        .remove("handlers")
        .unwrap_or_else(|| json!({}))
        .as_object()
        .cloned()
        .ok_or("definition JSON pointer `/handlers`: expected an object")?;
    if declared.contains_key("other") && !has_other {
        return Err(
            "definition JSON pointer `/handlers/other`: use `on_other` for reserved other handlers"
                .into(),
        );
    }
    if let Some(other) = object.remove("on_other") {
        let other = other
            .as_array()
            .cloned()
            .ok_or("definition JSON pointer `/on_other`: expected an array of handler objects")?;
        if declared
            .insert("other".into(), Value::Array(other))
            .is_some()
        {
            return Err(
                "definition JSON pointer `/on_other`: use `handlers.other` or `on_other`, not both"
                    .into(),
            );
        }
    }
    let normalized = handlers::validate_handler_file(Value::Object(declared), &buckets)?;
    let mut normalized = normalized.as_object().cloned().unwrap_or_default();
    let on_other = normalized.remove("other").unwrap_or_else(|| json!([]));
    if has_handlers {
        object.insert("handlers".into(), Value::Object(normalized));
    }
    if has_other {
        object.insert("on_other".into(), on_other);
    }
    Ok(())
}

fn map_none_route(
    bucket: &str,
    action: &str,
    grouped: &mut BTreeMap<String, Vec<Value>>,
    silent: &mut std::collections::BTreeSet<String>,
) -> Result<bool, String> {
    let handlers = grouped.entry(bucket.into()).or_default();
    if action == "none" {
        if !handlers.is_empty() || !silent.insert(bucket.into()) {
            return Err(
                "route none cannot be combined with another route for the same bucket".into(),
            );
        }
        return Ok(true);
    }
    if silent.contains(bucket) {
        return Err("route none cannot be combined with another route for the same bucket".into());
    }
    Ok(false)
}

#[cfg(test)]
#[path = "blueprint/tests.rs"]
mod tests;
