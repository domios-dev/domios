use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;

use crate::args::sentinel::SentinelAction;
use crate::client::SocketClient;
use crate::cmd::print::{fail, print_json};

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

fn render(job: &Value) {
    println!(
        "{} [{}] bucket={} every={}ms{}",
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
        job.pointer("/machine/last_bucket")
            .and_then(Value::as_str)
            .unwrap_or("unclassified"),
        job["interval_ms"],
        job["last_reason"]
            .as_str()
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default()
    );
}

pub fn handle_sentinel(client: &mut SocketClient, json_mode: bool, action: SentinelAction) {
    let (method, params) = request(
        action,
        std::env::var("CODETERM_TAB_ID").ok(),
        std::env::var("CODETERM_SESSION_BINDING_NONCE").ok(),
    )
    .unwrap_or_else(|error| fail(2, error));
    let result = client
        .call(method, Some(params))
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if json_mode {
        print_json(&result);
        return;
    }
    if let Some(jobs) = result.as_array() {
        if jobs.is_empty() {
            println!("No sentinels.");
        }
        for job in jobs {
            render(job);
        }
    } else {
        render(&result);
        if method == "sentinel.show" {
            println!(
                "Probe: {}\nQuestion: {}\nCreator: {}",
                result["probe"].as_str().unwrap_or("?"),
                result["question"].as_str().unwrap_or("?"),
                result["creator_tab_id"].as_str().unwrap_or("?")
            );
        }
    }
}

fn request(
    action: SentinelAction,
    caller: Option<String>,
    nonce: Option<String>,
) -> Result<(&'static str, Value), String> {
    let mut params = json!({ "caller_tab_id": caller, "session_binding_nonce": nonce });
    let method = match action {
        SentinelAction::Create {
            name,
            probe,
            question,
            buckets,
            every,
            routes,
            handlers_file,
            paused,
            max_transitions,
            max_actions,
            until,
            timeout,
            matches,
        } => {
            let (routes, handlers) = declared_outputs(routes, handlers_file.as_deref())?;
            params["create"] = json!({ "name": name, "probe": probe, "question": question, "buckets": buckets, "every": every, "routes": routes, "handlers": handlers, "paused": paused, "max_transitions": max_transitions, "max_actions": max_actions, "matches": assignments(matches)?, "until": until, "timeout": timeout });
            "sentinel.create"
        }
        SentinelAction::List => "sentinel.list",
        SentinelAction::Show { id } => {
            params["id"] = json!(id);
            "sentinel.show"
        }
        SentinelAction::Stop { id } => {
            params["id"] = json!(id);
            "sentinel.stop"
        }
    };
    Ok((method, params))
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct HandlerDefinition {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    parameters: HandlerParameters,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct HandlerParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
}

fn declared_outputs(
    routes: Vec<String>,
    path: Option<&std::path::Path>,
) -> Result<(BTreeMap<String, String>, Option<Value>), String> {
    if path.is_some() && !routes.is_empty() {
        return Err("--route and --handlers-file cannot be combined".into());
    }
    if let Some(path) = path {
        let mut raw = String::new();
        std::fs::File::open(path)
            .map_err(|error| error.to_string())?
            .take(65_537)
            .read_to_string(&mut raw)
            .map_err(|error| error.to_string())?;
        if raw.len() > 65_536 {
            return Err("handlers file exceeds 64 KiB".into());
        }
        let definitions: BTreeMap<String, Vec<HandlerDefinition>> =
            serde_json::from_str(&raw).map_err(|e| format!("invalid handlers file: {e}"))?;
        let mut ids = std::collections::BTreeSet::new();
        for handlers in definitions.values() {
            if handlers.len() > 16 {
                return Err("at most 16 handlers per bucket".into());
            }
            for handler in handlers {
                if !ids.insert(&handler.id) {
                    return Err("duplicate handler ID".into());
                }
                let parameters = &handler.parameters;
                if handler.kind == "command" {
                    let command = parameters
                        .command
                        .as_deref()
                        .ok_or("command text is required")?;
                    let label = parameters
                        .label
                        .as_deref()
                        .ok_or("command label is required")?;
                    let timeout = parameters
                        .timeout_ms
                        .ok_or("command timeout_ms is required")?;
                    if parameters.target.is_some()
                        || command.trim().is_empty()
                        || command.len() > 8192
                        || command.contains('\0')
                    {
                        return Err("command requires local fixed text of at most 8192 bytes without NUL and no target".into());
                    }
                    if label.trim().is_empty()
                        || label.len() > 256
                        || label.chars().any(char::is_control)
                    {
                        return Err("command label must be bounded display text".into());
                    }
                    #[cfg(windows)]
                    let runtime_ms = 10_000;
                    #[cfg(not(windows))]
                    let runtime_ms = 3_000;
                    if timeout == 0 || timeout > runtime_ms {
                        return Err("command timeout_ms exceeds the platform runtime".into());
                    }
                } else {
                    if parameters.command.is_some()
                        || parameters.label.is_some()
                        || parameters.timeout_ms.is_some()
                    {
                        return Err("command parameters require type=command".into());
                    }
                    match (handler.kind.as_str(), parameters.target.as_deref()) {
                        ("notify" | "stop", None) => {}
                        ("sentinel" | "send" | "cron", Some(target))
                            if !target.trim().is_empty() => {}
                        _ => return Err("unknown handler type or invalid parameters".into()),
                    }
                }
            }
        }
        if ids.len() > 64 {
            return Err("at most 64 handlers per sentinel".into());
        }
        return Ok((
            BTreeMap::new(),
            Some(serde_json::to_value(definitions).map_err(|e| e.to_string())?),
        ));
    }
    let mut grouped: BTreeMap<String, Vec<HandlerDefinition>> = BTreeMap::new();
    let mut single = BTreeMap::new();
    let mut repeated = false;
    for (index, route) in routes.into_iter().enumerate() {
        let (bucket, action) = route.split_once('=').ok_or("use bucket=action")?;
        repeated |= single.insert(bucket.into(), action.into()).is_some();
        let (kind, target) = action
            .split_once(':')
            .map(|(k, t)| (k, Some(t.to_string())))
            .unwrap_or((action, None));
        grouped
            .entry(bucket.into())
            .or_default()
            .push(HandlerDefinition {
                id: format!("handler_{}", index + 1),
                kind: kind.into(),
                parameters: HandlerParameters {
                    target,
                    ..Default::default()
                },
            });
    }
    if repeated {
        Ok((
            BTreeMap::new(),
            Some(serde_json::to_value(grouped).map_err(|e| e.to_string())?),
        ))
    } else {
        Ok((single, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_repeated_route_wire_preserves_order_and_stable_ids() {
        let (routes, handlers) =
            declared_outputs(vec!["done=notify".into(), "done=stop".into()], None).unwrap();
        assert!(routes.is_empty());
        assert_eq!(
            handlers.unwrap(),
            json!({"done":[
                {"id":"handler_1","type":"notify","parameters":{}},
                {"id":"handler_2","type":"stop","parameters":{}}
            ]})
        );
    }

    #[test]
    fn sentinel_handlers_file_refuses_mixed_duplicate_unknown_and_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        std::fs::write(
            &path,
            r#"{"done":[{"id":"h","type":"notify","parameters":{}}]}"#,
        )
        .unwrap();
        assert!(declared_outputs(Vec::new(), Some(&path)).is_ok());
        assert!(declared_outputs(vec!["done=stop".into()], Some(&path)).is_err());
        for raw in [
            r#"{"done":[{"id":"h","type":"notify","parameters":{}},{"id":"h","type":"stop","parameters":{}}]}"#,
            r#"{"done":[{"id":"h","type":"notify","parameters":{},"extra":true}]}"#,
            r#"{"done":[{"id":"h","type":"notify","parameters":{"extra":true}}]}"#,
            r#"{"done":[{"id":"h","type":"command","parameters":{}}]}"#,
            r#"{"done":[{"id":"h","type":"unknown","parameters":{}}]}"#,
        ] {
            std::fs::write(&path, raw).unwrap();
            assert!(declared_outputs(Vec::new(), Some(&path)).is_err());
        }
    }
    #[test]
    fn sentinel_create_rpc_request_preserves_declared_handlers_and_tab_nonce() {
        use clap::Parser;
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "Supplied name",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
            "--route",
            "done=notify",
            "--route",
            "done=sentinel:sentinel_child",
            "--paused",
            "--timeout",
            "1h",
            "--max-transitions",
            "7",
            "--max-actions",
            "9",
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("unexpected command")
        };
        let (method, params) = request(
            action,
            Some("supplied-creator".into()),
            Some("supplied-nonce".into()),
        )
        .unwrap();
        assert_eq!(method, "sentinel.create");
        assert_eq!(params["caller_tab_id"], "supplied-creator");
        assert_eq!(params["session_binding_nonce"], "supplied-nonce");
        let create = &params["create"];
        assert_eq!(create["name"], "Supplied name");
        assert_eq!(create["paused"], true);
        assert_eq!(create["max_transitions"], 7);
        assert_eq!(create["max_actions"], 9);
        assert_eq!(create["routes"], json!({}));
        assert_eq!(
            create["handlers"]["done"][1],
            json!({"id":"handler_2","type":"sentinel","parameters":{"target":"sentinel_child"}})
        );
        assert_eq!(create["timeout"], "1h");
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn sentinel_command_handlers_file_reaches_authenticated_create_request_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        let command = "printf '$model ${output} `raw`'";
        let definitions = json!({"done":[{"id":"fixed", "type":"command", "parameters":{"command":command, "label":"Fixed text", "timeout_ms":1000}}]});
        std::fs::write(&path, serde_json::to_vec(&definitions).unwrap()).unwrap();
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
            "--match",
            "done=done",
            "--timeout",
            "1h",
            "--handlers-file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("wrong command");
        };
        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "sentinel.create");
        assert_eq!(params["caller_tab_id"], "creator");
        assert_eq!(params["session_binding_nonce"], "nonce");
        assert_eq!(params["create"]["handlers"], definitions);
        assert_eq!(params["create"]["routes"], json!({}));
    }
}
