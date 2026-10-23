use serde_json::{json, Value};

use crate::args::blueprint::{BlueprintAction, BlueprintEscalationAction};
use crate::client::SocketClient;
use crate::cmd::print::{fail, print_json};
mod apply_output;
mod authoring;
mod call_view;
mod memory;
mod pipeline_spec;
mod recipe_files;
mod recipes;
pub use authoring::handle_local;

pub fn handle_recipe(json_mode: bool, name: Option<&str>) {
    recipes::print(json_mode, name);
}

fn read_text_input_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot read text input {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let limit = codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES;
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
    let calls = call_view::text(job);
    if !calls.is_empty() {
        println!("{calls}");
    }
    if job["saved_solution"] == true {
        if let Ok(body) = serde_json::to_string_pretty(&job["call_graph"]) {
            println!("{body}");
        }
        println!(
            "{} [callable] {}@{}",
            job["name"].as_str().unwrap_or("?"),
            job["id"].as_str().unwrap_or("?"),
            job["hash"].as_str().unwrap_or("?")
        );
        return;
    }
    if let Some(note) = job["note"].as_str().filter(|note| !note.is_empty()) {
        println!("Note: {note}");
    }
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
    let author_intent = match &action {
        BlueprintAction::New {
            intent: Some(intent),
            ..
        } => Some(intent.clone()),
        _ => None,
    };
    let checking = matches!(&action, BlueprintAction::Check { .. });
    let recipe_list = matches!(&action, BlueprintAction::Recipe { name: None });
    let apply_source = match &action {
        BlueprintAction::Apply { source, .. } => Some(source.clone()),
        _ => None,
    };
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
    let intent = params["intent"].as_str().unwrap_or("").to_owned();
    let recalled = client.call_detailed(method, Some(params));
    if let Some(intent) = author_intent {
        let response =
            memory::intent_response(&intent, recalled.map_err(|error| error.to_string()))
                .unwrap_or_else(|error| fail(2, error));
        if json_mode {
            print_json(&response);
        } else {
            print!("{}", memory::render_intent(&response));
        }
        return;
    }
    let mut result = recalled.unwrap_or_else(|error| {
        if let Some(source) = apply_source.as_deref() {
            authoring::fail_apply_error(source, &error, json_mode)
        }
        fail(1, error.to_string())
    });
    if checking {
        if json_mode {
            print_json(
                &json!({"ok":true,"diagnostics":result["diagnostics"],"definition":result["after"],"call_drift":result["call_drift"]}),
            );
        } else {
            if let Ok(diagnostics) = serde_json::from_value::<
                Vec<codeterm_blueprint_types::blueprint_document::Diagnostic>,
            >(result["diagnostics"].clone())
            {
                print!(
                    "{}",
                    codeterm_blueprint_types::blueprint_document::checker::render(
                        &diagnostics,
                        true
                    )
                );
            }
            print!("{}", call_view::drift(&result));
            println!("Valid Blueprint (host admission)");
        }
        return;
    }
    if !json_mode {
        print!("{}", call_view::drift(&result));
    }
    if method == "blueprint.recall" {
        if !recipe_list {
            result = memory::offers(result, &intent);
        }
        if recipe_list {
            let builtins = recipe_files::FILES
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>();
            if json_mode {
                print_json(&json!({"built_in":builtins,"stored":result}));
            } else {
                for name in builtins {
                    println!("{name}");
                }
                for entry in result.as_array().into_iter().flatten() {
                    println!(
                        "memory:{} {} (uses={} successes={})",
                        entry["id"].as_str().unwrap_or("?"),
                        entry["when_to_use"].as_str().unwrap_or("?"),
                        entry["uses"],
                        entry["successes"]
                    );
                }
            }
        } else if json_mode {
            print_json(&json!({"offers":result}));
        } else if let Some(offer) = result.as_array().and_then(|offers| offers.first()) {
            print!("{}", offer["yaml"].as_str().unwrap_or(""));
        } else {
            println!(
                "No proven graph matches this intent. Choose a built-in with blueprint recipe."
            );
        }
        return;
    }
    if method == "blueprint.keep" {
        if json_mode {
            print_json(&result)
        } else {
            println!("{}", memory::save_receipt(&result));
            if let Some(hash) = result["hash"].as_str() {
                println!("Hash: {hash}");
            }
        }
        return;
    }
    if method == "blueprint.export" {
        let recipe = serde_json::from_value(result)
            .unwrap_or_else(|error| fail(1, format!("invalid exported recipe: {error}")));
        let raw = codeterm_blueprint_types::blueprint_document::Document::from_recipe(&recipe)
            .and_then(|document| document.yaml())
            .unwrap_or_else(|error| fail(1, error));
        if let Some(path) = export_out {
            std::fs::write(&path, format!("{raw}\n"))
                .unwrap_or_else(|error| fail(1, error.to_string()));
        } else {
            println!("{raw}");
        }
        return;
    }
    if method == "blueprint.apply" || method == "blueprint.apply_recipe" {
        if json_mode {
            print_json(&result)
        } else {
            println!("{}", apply_output::summary(&result))
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
    if method == "blueprint.update" {
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
        if method == "blueprint.show" && result["saved_solution"] != true {
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
            let bundle = authoring::local_bundle(&file.to_string_lossy())?;
            let definition = bundle
                .document
                .bind(&set)
                .map_err(|errors| format!("{errors:?}"))?;
            params["local_callees"] =
                serde_json::to_value(bundle.local_callees).map_err(|e| e.to_string())?;
            params["recipe"] = json!({"schema_version":2,"definition":definition});
            params["set"] = json!([]);
            params["stopped"] = json!(!start);
            "blueprint.import"
        }
        BlueprintAction::Recipe { name: None } => {
            params["intent"] = json!("");
            "blueprint.recall"
        }
        BlueprintAction::Recipe { .. } => {
            return Err("named blueprint recipe is handled locally".into())
        }
        BlueprintAction::New {
            intent: Some(intent),
            limit,
            ..
        } => {
            if intent.trim().is_empty()
                || intent.len() > 4096
                || intent.contains('\0')
                || !(1..=20).contains(&limit)
            {
                return Err(
                    "Intent must be 1..4096 UTF-8 bytes without NUL; limit must be 1..20".into(),
                );
            }
            params["intent"] = json!(intent);
            params["cards"] = json!(true);
            params["limit"] = json!(limit);
            "blueprint.recall"
        }
        BlueprintAction::New {
            like: Some(intent), ..
        }
        | BlueprintAction::Apply {
            like: Some(intent), ..
        } => {
            params["intent"] = json!(intent);
            "blueprint.recall"
        }
        BlueprintAction::Keep { id, file, when } => {
            if let Some(file) = file {
                let bundle = authoring::local_bundle(&file.to_string_lossy())?;
                params["definition"] =
                    serde_json::to_value(bundle.document).map_err(|e| e.to_string())?;
                params["local_callees"] =
                    serde_json::to_value(bundle.local_callees).map_err(|e| e.to_string())?;
            } else {
                params["id"] = json!(id.ok_or("keep requires id or --file")?);
            }
            params["when"] =
                json!(codeterm_blueprint_types::blueprint_memory::validate_intents(&when)?);
            "blueprint.keep"
        }
        BlueprintAction::Apply {
            source,
            like: None,
            set,
            note,
            dry_run,
        } if source.starts_with("memory:") => {
            params["id"] = json!(source.trim_start_matches("memory:"));
            params["set"] = json!(set);
            params["note"] = json!(note);
            params["dry_run"] = json!(dry_run);
            "blueprint.apply_recipe"
        }
        BlueprintAction::Apply {
            source,
            like: None,
            set,
            note,
            dry_run,
        } => {
            let bundle = authoring::local_bundle(&source)?;
            if bundle.document.kind == codeterm_blueprint_types::blueprint_document::Kind::Pipeline
                && bundle.document.trigger.is_none()
                && bundle.document.result.port.is_some()
            {
                return Err("Callable Blueprints are saved with `blueprint keep --file`; reuse them via call".into());
            }
            let mut definition = bundle
                .document
                .bind(&set)
                .map_err(|errors| format!("{errors:?}"))?;
            params["local_callees"] =
                serde_json::to_value(bundle.local_callees).map_err(|e| e.to_string())?;
            if let Some(note) = note {
                definition["note"] = json!(note);
            }
            params["definition"] = definition;
            params["dry_run"] = json!(dry_run);
            "blueprint.apply"
        }
        BlueprintAction::Check {
            source, set, fix, ..
        } => {
            let bundle = authoring::local_bundle(&source)?;
            if fix
                && (bundle.document.nodes().any(|node| node.call.is_some())
                    || !bundle.document.imports.is_empty())
            {
                return Err("--fix on call documents requires host resolution and is not supported; check without --fix".into());
            }
            params["definition"] = if bundle.document.trigger.is_none()
                && bundle.document.result.port.is_some()
                && set.is_empty()
            {
                serde_json::to_value(&bundle.document).map_err(|e| e.to_string())?
            } else {
                bundle.document.bind(&set).map_err(|e| format!("{e:?}"))?
            };
            params["local_callees"] =
                serde_json::to_value(bundle.local_callees).map_err(|e| e.to_string())?;
            params["dry_run"] = json!(true);
            "blueprint.apply"
        }
        BlueprintAction::New { .. } | BlueprintAction::Schema => {
            return Err("Blueprint command is handled offline".into())
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
                || text.len() > codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES
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
            note,
            id,
            expect_version,
            from_file,
            cause,
            dry_run,
        } => {
            params["id"] = json!(id);
            params["expect_version"] = json!(expect_version);
            let bundle = authoring::local_bundle(&from_file.to_string_lossy())?;
            let mut definition = bundle
                .document
                .bind(&[])
                .map_err(|errors| format!("{errors:?}"))?;
            params["local_callees"] =
                serde_json::to_value(bundle.local_callees).map_err(|e| e.to_string())?;
            if let Some(note) = note {
                definition["note"] = json!(note);
            }
            params["definition"] = definition;
            params["cause"] = json!(cause);
            params["dry_run"] = json!(dry_run);
            "blueprint.update"
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

fn input_source_line(value: &Value) -> Option<String> {
    use codeterm_blueprint_types::sentinel_input::{display_name, DataRef, InputSpec};
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

#[cfg(test)]
#[path = "blueprint/tests.rs"]
mod tests;
