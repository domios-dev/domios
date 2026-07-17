use crate::client::SocketClient;
use crate::ServiceAction;

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    Canonical(String),
    Match(String),
    NotFound,
    Ambiguous(Vec<String>),
}

/// Pure resolution: given the user input and a fetched services-list JSON
/// payload, decide what id to use. Split out so the matching logic is
/// unit-testable without a live SocketClient.
pub fn resolve_service_outcome(input: &str, services_list: &serde_json::Value) -> ResolveOutcome {
    if input.starts_with("svc_") {
        return ResolveOutcome::Canonical(input.to_string());
    }
    let arr = services_list.as_array().cloned().unwrap_or_default();
    let matches: Vec<String> = arr
        .iter()
        .filter(|s| s.get("name").and_then(|v| v.as_str()) == Some(input))
        .filter_map(|s| s.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    match matches.len() {
        0 => ResolveOutcome::NotFound,
        1 => ResolveOutcome::Match(matches.into_iter().next().unwrap()),
        _ => ResolveOutcome::Ambiguous(matches),
    }
}

/// Resolve a user-supplied service identifier to the canonical `svc_*` id.
///
/// Service ids look like `svc_<name>_<hash>`. CLI users typically remember
/// the human name they passed to `service add`. If `input` doesn't already
/// look like a canonical id, fetch the full list and match by `name`.
fn resolve_service_id(client: &mut SocketClient, input: &str) -> String {
    if input.starts_with("svc_") {
        return input.to_string();
    }
    let val = match client.call("services.list", Some(serde_json::json!({}))) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "Error: services.list failed while resolving '{}': {}",
                input, e
            );
            std::process::exit(1);
        }
    };
    match resolve_service_outcome(input, &val) {
        ResolveOutcome::Canonical(s) | ResolveOutcome::Match(s) => s,
        ResolveOutcome::NotFound => {
            eprintln!("Error: no service named '{}'", input);
            std::process::exit(1);
        }
        ResolveOutcome::Ambiguous(ids) => {
            eprintln!(
                "Error: {} services match name '{}'. Use the full svc_* id:",
                ids.len(),
                input
            );
            for id in &ids {
                eprintln!("  {}", id);
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_service_outcome, service_add_json, ResolveOutcome};

    #[test]
    fn canonical_id_passes_through_unchanged() {
        let list = serde_json::json!([]);
        assert_eq!(
            resolve_service_outcome("svc_explorer_abc", &list),
            ResolveOutcome::Canonical("svc_explorer_abc".to_string())
        );
    }

    #[test]
    fn name_resolves_to_unique_match() {
        let list = serde_json::json!([
            {"id": "svc_explorer_abc", "name": "explorer-test", "kind": "daemon"},
            {"id": "svc_other_def", "name": "other", "kind": "daemon"},
        ]);
        assert_eq!(
            resolve_service_outcome("explorer-test", &list),
            ResolveOutcome::Match("svc_explorer_abc".to_string())
        );
    }

    #[test]
    fn unknown_name_returns_not_found() {
        let list = serde_json::json!([
            {"id": "svc_explorer_abc", "name": "explorer-test"},
        ]);
        assert_eq!(
            resolve_service_outcome("nope", &list),
            ResolveOutcome::NotFound
        );
    }

    #[test]
    fn duplicate_name_returns_ambiguous() {
        let list = serde_json::json!([
            {"id": "svc_explorer_abc", "name": "dup"},
            {"id": "svc_explorer_xyz", "name": "dup"},
        ]);
        match resolve_service_outcome("dup", &list) {
            ResolveOutcome::Ambiguous(ids) => assert_eq!(ids.len(), 2),
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn service_add_json_omits_token_without_push() {
        let response = serde_json::json!({
            "service": {"id": "svc_demo", "name": "demo"},
            "token": "secret-token"
        });

        let output = service_add_json(&response, false);

        assert_eq!(output["service"]["id"], "svc_demo");
        assert!(output.get("token").is_none());
    }

    #[test]
    fn service_add_json_retains_token_with_push() {
        let response = serde_json::json!({"token": "secret-token"});

        assert_eq!(service_add_json(&response, true)["token"], "secret-token");
    }
}

fn service_add_json(value: &serde_json::Value, push: bool) -> serde_json::Value {
    let mut value = value.clone();
    if !push {
        if let Some(object) = value.as_object_mut() {
            object.remove("token");
        }
    }
    value
}

pub fn handle_service(client: &mut SocketClient, json: bool, action: ServiceAction) {
    match action {
        ServiceAction::List { kind, enabled } => {
            let mut params = serde_json::json!({});
            if let Some(k) = kind {
                params["kind"] = serde_json::json!(k);
            }
            if let Some(e) = enabled {
                params["enabled"] = serde_json::json!(e);
            }
            match client.call("services.list", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() {
                            println!("No services registered.");
                        }
                        for s in arr {
                            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                            let enabled =
                                s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                            let url = s.get("url").and_then(|v| v.as_str()).unwrap_or("-");
                            let status = if enabled { "enabled" } else { "disabled" };
                            println!("{} {} [{}] kind={} url={}", id, name, status, kind, url);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ServiceAction::Add {
            name,
            kind,
            health_mode,
            url,
            host,
            port,
            process_label,
            script_path,
            health_path,
            health_json_field,
            health_ok_values,
            health_degraded_values,
            health_interval,
            description,
            push,
        } => {
            let mut params = serde_json::json!({
                "name": name,
                "kind": kind,
                "health_mode": health_mode,
                "health_interval": health_interval,
                "push_enabled": push,
            });
            if let Some(v) = url {
                params["url"] = serde_json::json!(v);
            }
            if let Some(v) = host {
                params["host"] = serde_json::json!(v);
            }
            if let Some(v) = port {
                params["port"] = serde_json::json!(v);
            }
            if let Some(v) = process_label {
                params["process_label"] = serde_json::json!(v);
            }
            if let Some(v) = script_path {
                params["script_path"] = serde_json::json!(v);
            }
            if let Some(v) = health_path {
                params["health_path"] = serde_json::json!(v);
            }
            if let Some(v) = health_json_field {
                params["health_json_field"] = serde_json::json!(v);
            }
            if let Some(v) = health_ok_values {
                params["health_ok_values"] = serde_json::json!(v);
            }
            if let Some(v) = health_degraded_values {
                params["health_degraded_values"] = serde_json::json!(v);
            }
            if let Some(v) = description {
                params["description"] = serde_json::json!(v);
            }
            match client.call("services.create", Some(params)) {
                Ok(val) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&service_add_json(&val, push)).unwrap()
                        );
                    } else {
                        let svc = val.get("service").unwrap_or(&val);
                        let token = val.get("token").and_then(|v| v.as_str()).unwrap_or("");
                        let svc_id = svc.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let svc_name = svc.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        println!("Created: {} ({})", svc_name, svc_id);
                        if push && !token.is_empty() {
                            println!();
                            println!("Push token (save this NOW — it will not be shown again):");
                            println!("  {}", token);
                            println!();
                            println!("Daemon should send this as: Authorization: Bearer <token>");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ServiceAction::Show { id } => {
            let id = resolve_service_id(client, &id);
            match client.call("services.get", Some(serde_json::json!({"id": id}))) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if val.is_null() {
                        eprintln!("Service not found: {}", id);
                        std::process::exit(1);
                    } else {
                        print!("{}", render_service_terse(&val));
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ServiceAction::Edit {
            id,
            name,
            url,
            health_path,
            health_json_field,
            health_ok_values,
            health_degraded_values,
            enabled,
        } => {
            let id = resolve_service_id(client, &id);
            let mut params = serde_json::json!({"id": id});
            if let Some(v) = name {
                params["name"] = serde_json::json!(v);
            }
            if let Some(v) = url {
                params["url"] = serde_json::json!(v);
            }
            if let Some(v) = health_path {
                params["health_path"] = serde_json::json!(v);
            }
            if let Some(v) = health_json_field {
                params["health_json_field"] = serde_json::json!(v);
            }
            if let Some(v) = health_ok_values {
                params["health_ok_values"] = serde_json::json!(v);
            }
            if let Some(v) = health_degraded_values {
                params["health_degraded_values"] = serde_json::json!(v);
            }
            if let Some(v) = enabled {
                params["enabled"] = serde_json::json!(v);
            }
            match client.call("services.update", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let n = val.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        println!("Updated: {}", n);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ServiceAction::Delete { id } => {
            let id = resolve_service_id(client, &id);
            match client.call("services.delete", Some(serde_json::json!({"id": id}))) {
                Ok(_) => println!("Deleted: {}", id),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ServiceAction::Token { id, rotate } => {
            let id = resolve_service_id(client, &id);
            if rotate {
                match client.call(
                    "services.regenerate_token",
                    Some(serde_json::json!({"id": id})),
                ) {
                    Ok(val) => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        } else {
                            let token = val.get("token").and_then(|v| v.as_str()).unwrap_or("");
                            println!("New token (save this NOW — it will not be shown again):");
                            println!("  {}", token);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Tokens are only retrievable at create time or via --rotate (which generates a new one and invalidates the old).");
                eprintln!("Use: codeterm service token {} --rotate", id);
                std::process::exit(1);
            }
        }
    }
}

/// Compact human render of a single service. Skips empty/null fields so an
/// agent reading `service show` doesn't pay for a full pretty-printed JSON
/// dump (use --json for that). Returns the block (trailing newline included).
fn render_service_terse(val: &serde_json::Value) -> String {
    use std::fmt::Write;
    let s = |k: &str| val.get(k).and_then(|v| v.as_str());
    let mut out = String::new();
    let id = s("id").unwrap_or("?");
    let name = s("name").unwrap_or("?");
    let kind = s("kind").unwrap_or("?");
    let status = if val
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        "enabled"
    } else {
        "disabled"
    };
    let _ = writeln!(out, "{} {} [{}] kind={}", id, name, status, kind);

    if let Some(d) = s("description") {
        let _ = writeln!(out, "  {}", d);
    }

    let endpoint = if let Some(u) = s("url") {
        Some(u.to_string())
    } else {
        s("host").map(|h| match val.get("port").and_then(|v| v.as_u64()) {
            Some(p) => format!("{}:{}", h, p),
            None => h.to_string(),
        })
    };
    if let Some(ep) = endpoint {
        match s("health_path") {
            Some(hp) => {
                let _ = writeln!(out, "  endpoint: {} (health {})", ep, hp);
            }
            None => {
                let _ = writeln!(out, "  endpoint: {}", ep);
            }
        }
    }

    if let Some(hstatus) = val
        .get("health")
        .and_then(|h| h.get("status"))
        .and_then(|v| v.as_str())
    {
        let mode = s("health_mode").unwrap_or("?");
        match val
            .get("health")
            .and_then(|h| h.get("latency_ms"))
            .and_then(|v| v.as_u64())
        {
            Some(lat) => {
                let _ = writeln!(out, "  health: {} (mode={}, {}ms)", hstatus, mode, lat);
            }
            None => {
                let _ = writeln!(out, "  health: {} (mode={})", hstatus, mode);
            }
        }
    }

    if kind == "cron" {
        if let Some(sch) = s("schedule") {
            let last = s("last_run_status").unwrap_or("-");
            match val.get("next_run").and_then(|v| v.as_i64()) {
                Some(n) => {
                    let _ = writeln!(out, "  schedule: {}  next={}  last={}", sch, n, last);
                }
                None => {
                    let _ = writeln!(out, "  schedule: {}  last={}", sch, last);
                }
            }
        }
    }

    if val
        .get("push_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let _ = writeln!(out, "  push: enabled");
    }
    out
}

#[cfg(test)]
mod terse_tests {
    use super::render_service_terse;

    #[test]
    fn daemon_show_is_compact_and_skips_empty_fields() {
        let val = serde_json::json!({
            "id": "svc_abc", "name": "API", "kind": "daemon", "enabled": true,
            "url": "http://localhost:8080", "health_path": "/health",
            "health_mode": "http", "push_enabled": true,
            "health": { "status": "healthy", "latency_ms": 42 },
            // noise that must NOT appear in terse output:
            "registered_at": 1700000000, "updated_at": 1700000001, "health_ttl": 30
        });
        let out = render_service_terse(&val);
        assert!(
            out.starts_with("svc_abc API [enabled] kind=daemon\n"),
            "got: {out}"
        );
        assert!(out.contains("endpoint: http://localhost:8080 (health /health)"));
        assert!(out.contains("health: healthy (mode=http, 42ms)"));
        assert!(out.contains("push: enabled"));
        assert!(!out.contains("registered_at"));
        assert!(!out.contains("1700000000"));
        assert!(!out.contains("health_ttl"));
        assert!(!out.contains("schedule:"));
    }

    #[test]
    fn cron_show_includes_schedule_disabled_when_off() {
        let val = serde_json::json!({
            "id": "cron_x", "name": "Nightly", "kind": "cron", "enabled": false,
            "schedule": "0 3 * * *", "last_run_status": "ok",
        });
        let out = render_service_terse(&val);
        assert!(
            out.starts_with("cron_x Nightly [disabled] kind=cron\n"),
            "got: {out}"
        );
        assert!(out.contains("schedule: 0 3 * * *  last=ok"));
    }
}
