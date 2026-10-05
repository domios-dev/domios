use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::args::sentinel::{SentinelAction, SentinelEscalationAction};
use crate::client::SocketClient;
use crate::cmd::print::{fail, print_json};
mod handlers;
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

fn render_history(history: &Value) {
    println!(
        "{} history: current version {}{}",
        history["sentinel_id"].as_str().unwrap_or("?"),
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

fn escalation_resolved_message(transition_id: Option<&str>, result: &Value) -> String {
    format!(
        "Resolved escalation {} for {}.",
        transition_id.unwrap_or("?"),
        result["id"].as_str().unwrap_or("?")
    )
}

pub fn handle_sentinel(client: &mut SocketClient, json_mode: bool, action: SentinelAction) {
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
    if json_mode {
        print_json(&result);
        return;
    }
    if method.starts_with("sentinel.validate.") {
        println!("{}", preview_summary(&result));
        return;
    }
    if method == "sentinel.history" {
        render_history(&result);
        return;
    }
    if method == "sentinel.update" {
        println!(
            "Updated {} to definition version {}.",
            result["id"].as_str().unwrap_or("?"),
            result["definition_version"]
        );
        return;
    }
    if method == "sentinel.escalation.resolve" {
        println!(
            "{}",
            escalation_resolved_message(transition_id.as_deref(), &result)
        );
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
            let source = result
                .pointer("/observe/kind")
                .and_then(Value::as_str)
                .zip(result.pointer("/observe/id").and_then(Value::as_str))
                .map(|(kind, id)| format!("Observe: {kind}:{id}"))
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
    action: SentinelAction,
    caller: Option<String>,
    nonce: Option<String>,
) -> Result<(&'static str, Value), String> {
    let mut params = json!({ "caller_tab_id": caller, "session_binding_nonce": nonce });
    let method = match action {
        SentinelAction::Recipe { .. } => return Err("sentinel recipe is handled locally".into()),
        SentinelAction::Create {
            name,
            probe,
            observe,
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
            dry_run,
        } => {
            let (routes, handlers) =
                handlers::declared_outputs(routes, handlers_file.as_deref(), &buckets)?;
            let observe = observe
                .map(|source| observation_binding(&source))
                .transpose()?;
            params["create"] = json!({ "name": name, "probe": probe, "observe": observe, "question": question, "buckets": buckets, "every": every, "routes": routes, "handlers": handlers, "rearm_on_change": rearm_on_change, "escalation_timeout_ms": escalation_timeout_ms, "paused": paused, "max_transitions": max_transitions, "max_actions": max_actions, "matches": assignments(matches)?, "until": until, "timeout": timeout });
            if dry_run {
                "sentinel.validate.create"
            } else {
                "sentinel.create"
            }
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
        SentinelAction::Update {
            id,
            expect_version,
            from_file,
            cause,
            dry_run,
        } => {
            params["id"] = json!(id);
            params["expect_version"] = json!(expect_version);
            params["update"] = read_update(&from_file, cause.as_deref())?;
            if dry_run {
                "sentinel.validate.update"
            } else {
                "sentinel.update"
            }
        }
        SentinelAction::History { id } => {
            params["id"] = json!(id);
            "sentinel.history"
        }
        SentinelAction::Escalation { action } => match action {
            SentinelEscalationAction::Resolve { id, transition_id } => {
                params["id"] = json!(id);
                params["transition_id"] = json!(transition_id);
                "sentinel.escalation.resolve"
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

fn preview_summary(job: &Value) -> String {
    let mut lines = vec![format!(
        "Sentinel graph: {}",
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
    if let Some(handlers) = job["on_other"]
        .as_array()
        .filter(|handlers| !handlers.is_empty())
    {
        lines.push("  other:".into());
        for handler in handlers {
            let action = serde_json::to_string(&handler["action"]).unwrap_or_else(|_| "?".into());
            lines.push(format!(
                "    {} -> {action}",
                handler["id"].as_str().unwrap_or("?")
            ));
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
mod tests {
    use super::*;

    #[test]
    fn sentinel_repeated_route_wire_preserves_order_and_stable_ids() {
        let (routes, handlers) = handlers::declared_outputs(
            vec!["done=notify".into(), "done=stop".into()],
            None,
            &["done".into()],
        )
        .unwrap();
        assert!(routes.is_empty());
        assert_eq!(
            handlers.unwrap(),
            json!({"done":[
                {"id":"notify-on-done","type":"notify","parameters":{}},
                {"id":"stop-on-done","type":"stop","parameters":{}}
            ]})
        );
    }

    #[test]
    fn sentinel_none_route_is_an_empty_handler_array_and_can_mix_with_routes() {
        let quiet = ["quiet".into()];
        let (routes, handlers) =
            handlers::declared_outputs(vec!["quiet=none".into()], None, &quiet).unwrap();
        assert_eq!(routes, BTreeMap::from([("quiet".into(), "none".into())]));
        assert!(handlers.is_none());

        let buckets = ["quiet".into(), "done".into()];
        let (routes, handlers) = handlers::declared_outputs(
            vec![
                "quiet=none".into(),
                "done=notify".into(),
                "done=stop".into(),
            ],
            None,
            &buckets,
        )
        .unwrap();
        assert!(routes.is_empty());
        let handlers = handlers.unwrap();
        assert_eq!(handlers["quiet"], json!([]));
        assert_eq!(handlers["done"].as_array().unwrap().len(), 2);

        assert!(handlers::declared_outputs(
            vec!["done=none".into(), "done=notify".into()],
            None,
            &["done".into()]
        )
        .is_err());
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
        assert!(handlers::declared_outputs(Vec::new(), Some(&path), &["done".into()]).is_ok());
        assert!(handlers::declared_outputs(
            vec!["done=stop".into()],
            Some(&path),
            &["done".into()]
        )
        .is_err());
        for raw in [
            r#"{"done":[{"id":"h","type":"notify","parameters":{}},{"id":"h","type":"stop","parameters":{}}]}"#,
            r#"{"done":[{"id":"h","type":"notify","parameters":{},"extra":true}]}"#,
            r#"{"done":[{"id":"h","type":"notify","parameters":{"extra":true}}]}"#,
            r#"{"done":[{"id":"h","type":"command","parameters":{}}]}"#,
            r#"{"done":[{"id":"h","type":"unknown","parameters":{}}]}"#,
        ] {
            std::fs::write(&path, raw).unwrap();
            assert!(handlers::declared_outputs(Vec::new(), Some(&path), &["done".into()]).is_err());
        }
    }

    #[test]
    fn sentinel_handlers_file_accepts_respond_prompt_kind_parameter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        std::fs::write(
            &path,
            r#"{"prompt":[{"id":"answer","type":"respond_prompt","parameters":{"target":"permission"}}]}"#,
        )
        .unwrap();

        let (_, handlers) =
            handlers::declared_outputs(Vec::new(), Some(&path), &["prompt".into()]).unwrap();

        assert_eq!(
            handlers.unwrap()["prompt"][0],
            json!({"id":"answer","type":"respond_prompt","parameters":{"target":"permission"}})
        );
        for parameters in ["{}", r#"{"target":"  "}"#, r#"{"target":"permission\n"}"#] {
            std::fs::write(
                &path,
                format!(r#"{{"prompt":[{{"id":"answer","type":"respond_prompt","parameters":{parameters}}}]}}"#),
            )
            .unwrap();
            assert!(
                handlers::declared_outputs(Vec::new(), Some(&path), &["prompt".into()]).is_err()
            );
        }
    }

    #[test]
    fn update_handler_definitions_generate_ids_for_buckets_and_other() {
        let mut definition = json!({
            "buckets": ["passed"],
            "handlers": {"passed": [{"type":"notify", "parameters":{}}]},
            "on_other": [{"type":"escalate", "parameters":{"target":"creator"}}]
        });

        normalize_update_handlers(definition.as_object_mut().unwrap()).unwrap();

        assert_eq!(
            definition["handlers"]["passed"][0]["id"],
            "notify-on-passed"
        );
        assert_eq!(
            definition["on_other"][0]["id"],
            "escalate-to-owner-on-other"
        );
    }

    #[test]
    fn sentinel_escalation_resolve_message_prints_requested_transition_id() {
        assert_eq!(
            escalation_resolved_message(Some("transition_9"), &json!({"id":"sentinel_a"})),
            "Resolved escalation transition_9 for sentinel_a."
        );
    }

    #[test]
    fn sentinel_escalate_handlers_file_accepts_reserved_other_handler() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        std::fs::write(
            &path,
            r#"{"other":[{"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}}]}"#,
        )
        .unwrap();

        let (_, handlers) = handlers::declared_outputs(Vec::new(), Some(&path), &[]).unwrap();

        assert_eq!(
            handlers.unwrap()["other"][0],
            json!({"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}})
        );
    }

    #[test]
    fn sentinel_escalate_resolve_request_preserves_stage_and_transition_ids() {
        let action = SentinelAction::Escalation {
            action: SentinelEscalationAction::Resolve {
                id: "sentinel_a".into(),
                transition_id: "transition_9".into(),
            },
        };

        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();

        assert_eq!(method, "sentinel.escalation.resolve");
        assert_eq!(params["id"], "sentinel_a");
        assert_eq!(params["transition_id"], "transition_9");
    }

    #[test]
    fn create_dry_run_uses_validation_rpc() {
        use clap::Parser;

        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "ci-green-watch",
            "--probe",
            "status",
            "--question",
            "classify-result",
            "--buckets",
            "passed",
            "--match",
            "passed=.*",
            "--dry-run",
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("unexpected command")
        };

        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();

        assert_eq!(method, "sentinel.validate.create");
        assert_eq!(params["create"]["name"], "ci-green-watch");
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
            "--rearm-on-change",
            "done",
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
        assert_eq!(create["rearm_on_change"], json!(["done"]));
        assert_eq!(
            create["handlers"]["done"][1],
            json!({"id":"activate-sentinel-child-on-done","type":"sentinel","parameters":{"target":"sentinel_child"}})
        );
        assert_eq!(create["timeout"], "1h");
    }

    #[test]
    fn sentinel_update_cli_json_round_trip_preserves_definition_and_history_request() {
        use clap::Parser;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("definition.json");
        let definition = json!({
            "name": "Updated graph",
            "question": "Did it finish?",
            "buckets": ["done"],
            "every": "30s",
            "routes": {"done": "notify"},
            "matches": {"done": "^done$"},
            "until": "done",
            "on_other": [],
            "max_transitions": 8,
            "max_actions": 12,
            "note": "new completion condition"
        });
        std::fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "update",
            "sentinel_a",
            "--expect-version",
            "3",
            "--from-file",
            path.to_str().unwrap(),
            "--cause",
            "transition_2",
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("unexpected command")
        };
        let (method, params) =
            request(action, Some("editor".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "sentinel.update");
        assert_eq!(params["caller_tab_id"], "editor");
        assert_eq!(params["session_binding_nonce"], "nonce");
        assert_eq!(params["expect_version"], 3);
        let mut expected = definition;
        expected["cause"] = json!("transition_2");
        assert_eq!(params["update"], expected);

        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "update",
            "sentinel_a",
            "--expect-version",
            "3",
            "--from-file",
            path.to_str().unwrap(),
            "--dry-run",
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("unexpected command")
        };
        let (method, _) = request(action, Some("editor".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "sentinel.validate.update");

        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "history",
            "sentinel_a",
        ])
        .unwrap();
        let crate::args::cli::Commands::Sentinel { action } = cli.command else {
            panic!("unexpected command")
        };
        let (method, params) = request(action, Some("reader".into()), None).unwrap();
        assert_eq!(method, "sentinel.history");
        assert_eq!(params["id"], "sentinel_a");
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn sentinel_observe_request_roundtrips_tab_and_cron_run_bindings() {
        for (source, expected) in [
            ("tab:bound-agent", json!({"kind":"tab", "id":"bound-agent"})),
            (
                "cron-run:nightly",
                json!({"kind":"cron_run", "id":"nightly"}),
            ),
        ] {
            let cli = crate::args::cli::Cli::try_parse_from([
                "codeterm",
                "sentinel",
                "create",
                "--name",
                "agent-state-watch",
                "--observe",
                source,
                "--question",
                "tab state?",
                "--buckets",
                "waiting",
                "--match",
                "waiting=.*",
                "--json",
            ])
            .unwrap();
            let crate::args::cli::Commands::Sentinel { action } = cli.command else {
                panic!("wrong command")
            };
            let (method, params) =
                request(action, Some("creator".into()), Some("nonce".into())).unwrap();
            assert_eq!(method, "sentinel.create");
            assert_eq!(params["create"]["probe"], Value::Null);
            assert_eq!(params["create"]["observe"], expected);
            let encoded = serde_json::to_string(&params["create"]).unwrap();
            let decoded: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, params["create"]);
        }
    }

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
            "--name",
            "command-result-watch",
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
