use std::io::Read;

use serde_json::{json, Value};

use crate::args::radar::RadarAction;
use crate::client::RpcClient;
use crate::cmd::plan::{call_plan_rpc, inject_trusted_session};
use crate::cmd::print::{exit_with_cli_error, CliError};

fn read_path_or_stdin(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {e}"))?;
        Ok(buf)
    } else {
        crate::local_admission::read_local_text(path).map_err(|e| format!("failed to read {path}: {e}"))
    }
}

fn resolve_text(value: Option<String>, file: Option<String>) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

/// The workspace a local command addresses; a mesh peer has no local cwd to offer.
fn workspace(client: &dyn RpcClient, explicit: Option<String>) -> Option<String> {
    explicit.or_else(|| {
        (!client.is_remote())
            .then(std::env::current_dir)
            .and_then(Result::ok)
            .map(|path| path.display().to_string())
    })
}

fn caller_tab(client: &dyn RpcClient) -> Option<String> {
    if client.is_remote() {
        return None;
    }
    crate::nonempty_env("CODETERM_TAB_ID")
}

pub fn handle_radar(client: &mut dyn RpcClient, json: bool, action: RadarAction) {
    let val =
        run_radar(client, action).unwrap_or_else(|e| exit_with_cli_error(json, &CliError::new(e)));
    if json {
        println!("{}", crate::cmd::output::encode(&val).unwrap_or_default());
    } else {
        print!("{}", render_radar_result(&val));
    }
}

pub(crate) fn run_radar(client: &mut dyn RpcClient, action: RadarAction) -> Result<Value, String> {
    match action {
        RadarAction::Add {
            title,
            body,
            body_file,
            tag,
            source_provider,
            source_id,
            source_url,
            project,
        } => {
            let project = workspace(client, project).ok_or(
                "radar add over mesh needs --project naming the workspace on the target machine",
            )?;
            let mut params = json!({ "project": project, "title": title, "tags": tag });
            if let Some(body) = resolve_text(body, body_file)? {
                params["body"] = json!(body);
            }
            for (key, value) in [
                ("source_provider", source_provider),
                ("source_id", source_id),
                ("source_url", source_url),
            ] {
                if let Some(value) = value {
                    params[key] = json!(value);
                }
            }
            call_plan_rpc(client, "radar.add", params)
        }
        RadarAction::List {
            status,
            tag,
            provider,
            query,
            project,
            all_workspaces,
        } => {
            let mut params = json!({});
            if !all_workspaces {
                let project = workspace(client, project)
                    .ok_or("radar list over mesh needs --project or --all-workspaces")?;
                params["project"] = json!(project);
            }
            for (key, value) in [
                ("status", status),
                ("tag", tag),
                ("provider", provider),
                ("query", query),
            ] {
                if let Some(value) = value {
                    params[key] = json!(value);
                }
            }
            call_plan_rpc(client, "radar.list", params)
        }
        RadarAction::Show { item_id } => {
            call_plan_rpc(client, "radar.show", json!({ "item_id": item_id }))
        }
        RadarAction::Edit {
            item_id,
            title,
            body,
            body_file,
            tag,
            clear_tags,
        } => {
            let mut params = json!({ "item_id": item_id });
            if let Some(title) = title {
                params["title"] = json!(title);
            }
            if let Some(body) = resolve_text(body, body_file)? {
                params["body"] = json!(body);
            }
            if clear_tags || !tag.is_empty() {
                params["tags"] = json!(tag);
            }
            call_plan_rpc(client, "radar.update", params)
        }
        RadarAction::Drop { item_id } => {
            call_plan_rpc(client, "radar.drop", json!({ "item_id": item_id }))
        }
        RadarAction::Pull {
            item_id,
            plan,
            kind,
            sections,
        } => {
            let mut params = json!({ "item_id": item_id, "plan_id": plan });
            if let Some(kind) = kind {
                params["type"] = json!(kind);
            }
            if let Some(sections) = sections {
                params["sections"] = json!(sections);
            }
            if let Some(caller) = caller_tab(client) {
                params["caller_tab_id"] = json!(caller);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "radar.pull", params)
        }
    }
}

fn item_line(item: &Value) -> String {
    let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("");
    let mut line = format!("{}\t[{}]\t{}", field("id"), field("status"), field("title"));
    let tags: Vec<&str> = item
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !tags.is_empty() {
        line.push_str(&format!("\t#{}", tags.join(" #")));
    }
    if let Some(source) = item.get("source") {
        let provider = source.get("provider").and_then(Value::as_str).unwrap_or("");
        let id = source
            .get("external_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        line.push_str(&format!("\t{provider}:{id}"));
    }
    if let Some(pull) = item.get("pulled") {
        let plan = pull.get("plan_id").and_then(Value::as_str).unwrap_or("");
        let task = pull.get("task_id").and_then(Value::as_str).unwrap_or("");
        line.push_str(&format!("\t-> {plan}/{task}"));
    }
    line
}

pub(crate) fn render_radar_result(val: &Value) -> String {
    if let Some(items) = val.get("items").and_then(Value::as_array) {
        if items.is_empty() {
            return "(no radar items)\n".into();
        }
        return items.iter().map(|item| item_line(item) + "\n").collect();
    }
    if let Some(item) = val.get("dropped") {
        return format!("Dropped {}\n", item_line(item));
    }
    let Some(item) = val.get("item") else {
        return format!("{val}\n");
    };
    let mut out = match val.get("created").and_then(Value::as_bool) {
        Some(true) => format!("Added {}\n", item_line(item)),
        Some(false) => format!("Refreshed {}\n", item_line(item)),
        None => format!("{}\n", item_line(item)),
    };
    if val.get("task_id").is_none() {
        if let Some(body) = item.get("body").and_then(Value::as_str) {
            if !body.trim().is_empty() && val.get("created").is_none() {
                out.push_str(&format!("\n{}\n", body.trim_end()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
