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
            eprintln!("Error: services.list failed while resolving '{}': {}", input, e);
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
                ids.len(), input
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
    use super::{resolve_service_outcome, ResolveOutcome};

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
}

pub fn handle_service(client: &mut SocketClient, json: bool, action: ServiceAction) {
    match action {
        ServiceAction::List { kind, enabled } => {
            let mut params = serde_json::json!({});
            if let Some(k) = kind { params["kind"] = serde_json::json!(k); }
            if let Some(e) = enabled { params["enabled"] = serde_json::json!(e); }
            match client.call("services.list", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(arr) = val.as_array() {
                        if arr.is_empty() { println!("No services registered."); }
                        for s in arr {
                            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                            let enabled = s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                            let url = s.get("url").and_then(|v| v.as_str()).unwrap_or("-");
                            let status = if enabled { "enabled" } else { "disabled" };
                            println!("{} {} [{}] kind={} url={}", id, name, status, kind, url);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Add { name, kind, health_mode, url, host, port, process_label, script_path, health_path, health_interval, description, push } => {
            let mut params = serde_json::json!({
                "name": name,
                "kind": kind,
                "health_mode": health_mode,
                "health_interval": health_interval,
                "push_enabled": push,
            });
            if let Some(v) = url { params["url"] = serde_json::json!(v); }
            if let Some(v) = host { params["host"] = serde_json::json!(v); }
            if let Some(v) = port { params["port"] = serde_json::json!(v); }
            if let Some(v) = process_label { params["process_label"] = serde_json::json!(v); }
            if let Some(v) = script_path { params["script_path"] = serde_json::json!(v); }
            if let Some(v) = health_path { params["health_path"] = serde_json::json!(v); }
            if let Some(v) = description { params["description"] = serde_json::json!(v); }
            match client.call("services.create", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
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
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
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
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Edit { id, name, url, enabled } => {
            let id = resolve_service_id(client, &id);
            let mut params = serde_json::json!({"id": id});
            if let Some(v) = name { params["name"] = serde_json::json!(v); }
            if let Some(v) = url { params["url"] = serde_json::json!(v); }
            if let Some(v) = enabled { params["enabled"] = serde_json::json!(v); }
            match client.call("services.update", Some(params)) {
                Ok(val) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        let n = val.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        println!("Updated: {}", n);
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Delete { id } => {
            let id = resolve_service_id(client, &id);
            match client.call("services.delete", Some(serde_json::json!({"id": id}))) {
                Ok(_) => println!("Deleted: {}", id),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        ServiceAction::Token { id, rotate } => {
            let id = resolve_service_id(client, &id);
            if rotate {
                match client.call("services.regenerate_token", Some(serde_json::json!({"id": id}))) {
                    Ok(val) => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&val).unwrap());
                        } else {
                            let token = val.get("token").and_then(|v| v.as_str()).unwrap_or("");
                            println!("New token (save this NOW — it will not be shown again):");
                            println!("  {}", token);
                        }
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            } else {
                eprintln!("Tokens are only retrievable at create time or via --rotate (which generates a new one and invalidates the old).");
                eprintln!("Use: codeterm service token {} --rotate", id);
                std::process::exit(1);
            }
        }
    }
}
