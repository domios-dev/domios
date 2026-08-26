use crate::{
    cmd::{
        delayed::parse_schedule,
        send_text::{actions_to_json, parse_actions_spec, resolve_send_text_input},
        team::split_provider_model,
    },
    AgentAction, MemAction, MemGraphAction, ServiceAction,
};
use serde_json::json;

use super::unsupported;

/// `team pair` is the sole Team action with a REST form. The body is the one
/// `orchestration new` posted; the socket-only `spawned_by_tab_id` stays local.
pub(super) fn team_pair_to_api(
    orchestrator: &str,
    planner: &str,
    workspace: &Option<String>,
    title: &Option<String>,
) -> (String, String, Option<String>) {
    let (orchestrator_agent, orchestrator_model) = split_provider_model(orchestrator);
    let (planner_agent, planner_model) = split_provider_model(planner);
    let mut body = json!({
        "orchestrator_agent": orchestrator_agent,
        "planner_agent": planner_agent,
    });
    if let Some(m) = orchestrator_model {
        body["orchestrator_model"] = json!(m);
    }
    if let Some(m) = planner_model {
        body["planner_model"] = json!(m);
    }
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    if let Some(t) = title {
        body["title"] = json!(t);
    }
    (
        "POST".into(),
        "/api/orchestration/new".into(),
        Some(body.to_string()),
    )
}

/// Resolve free-text content for mesh relay from (inline value, file, `-`=stdin).
/// File wins; a literal `-` reads stdin; otherwise the value is cloned.
fn resolve_remote_text(value: &Option<String>, file: &Option<String>) -> Option<String> {
    use std::io::Read as _;
    let read = |path: &str| -> String {
        let res = if path == "-" {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map(|_| buf)
                .map_err(|e| format!("failed to read stdin: {}", e))
        } else {
            std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))
        };
        match res {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    };
    if let Some(path) = file {
        return Some(read(path));
    }
    match value {
        Some(v) if v == "-" => Some(read("-")),
        other => other.clone(),
    }
}

pub(crate) fn send_text_to_api<R: std::io::Read>(
    text: &Option<String>,
    tab: &Option<String>,
    from_file: &Option<String>,
    from_pad: &Option<String>,
    from_stdin: bool,
    actions: &Option<String>,
    stdin: &mut R,
) -> (String, String, Option<String>) {
    let p = tab.as_deref().unwrap_or("general");
    let source = std::env::var("CODETERM_TAB_ID").unwrap_or_else(|_| "unknown".to_string());
    let (body_text, input_source) = match resolve_send_text_input(
        text.clone(),
        from_file.clone(),
        from_pad.clone(),
        from_stdin,
        stdin,
    ) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let parsed_actions = actions
        .as_deref()
        .map(parse_actions_spec)
        .transpose()
        .unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        });
    if body_text.trim().is_empty() && parsed_actions.is_none() {
        eprintln!("Error: send payload is empty (mesh path, source: {input_source})");
        eprintln!("hint: --stdin via heredoc may have dropped under your shell wrapper;");
        eprintln!("      try --from-file <path> with a real tempfile.");
        eprintln!("      to send a bare Enter, pass --actions enter.");
        std::process::exit(2);
    }
    if !body_text.trim().is_empty() {
        println!(
            "{}",
            crate::cmd::resolved_text::resolved_text_echo(&body_text)
        );
    }
    let mut body = serde_json::json!({"source_tab": source});
    if !body_text.is_empty() {
        body["text"] = serde_json::json!(body_text);
    }
    if let Some(actions) = parsed_actions {
        body["actions"] = actions_to_json(&actions);
    } else {
        body["text"] = serde_json::json!(body_text);
        body["enter"] = serde_json::json!(true);
    }
    (
        "POST".into(),
        format!("/api/tabs/{}/send", p),
        Some(body.to_string()),
    )
}

pub(crate) fn delayed_create_to_api<R: std::io::Read>(
    text: &Option<String>,
    tab: &Option<String>,
    from_file: &Option<String>,
    from_pad: &Option<String>,
    from_stdin: bool,
    delay: &Option<String>,
    at: &Option<String>,
    stdin: &mut R,
) -> (String, String, Option<String>) {
    let p = tab.as_deref().unwrap_or("general");
    let (body_text, input_source) = match resolve_send_text_input(
        text.clone(),
        from_file.clone(),
        from_pad.clone(),
        from_stdin,
        stdin,
    ) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    if body_text.trim().is_empty() {
        eprintln!("Error: delayed message payload is empty (mesh path, source: {input_source})");
        std::process::exit(2);
    }
    let scheduled_at_ms = match parse_schedule(delay.clone(), at.clone(), now_ms()) {
        Ok(ms) => ms,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        }
    };
    let body = serde_json::json!({
        "text": body_text,
        "scheduledAtMs": scheduled_at_ms,
    });
    (
        "POST".into(),
        format!("/api/tabs/{}/delayed-messages", p),
        Some(body.to_string()),
    )
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn mem_to_api(action: &MemAction) -> (String, String, Option<String>) {
    let enc = urlencoding::encode;
    match action {
        MemAction::Search { query, kind, area, scope, project, limit, .. } => {
            let mut path = format!("/api/mem/search?q={}&limit={}", enc(query), limit);
            if let Some(k) = kind { path.push_str(&format!("&kind={}", enc(k))); }
            if let Some(a) = area { path.push_str(&format!("&area={}", enc(a))); }
            if let Some(s) = scope { path.push_str(&format!("&scope={}", enc(s))); }
            if let Some(p) = project { path.push_str(&format!("&project={}", enc(p))); }
            ("GET".into(), path, None)
        }
        MemAction::Get { id } => ("GET".into(), format!("/api/mem/items/{}", enc(id)), None),
        MemAction::Delete { id } => ("DELETE".into(), format!("/api/mem/items/{}", enc(id)), None),
        MemAction::Forget { area, yes } => {
            if let Err(e) = crate::cmd::mem::item::validate_mem_forget_confirmation(area, *yes) {
                eprintln!("Error: {}", e);
                std::process::exit(2);
            }
            ("DELETE".into(), format!("/api/mem/forget/{}", enc(area)), None)
        }
        MemAction::History { id } => ("GET".into(), format!("/api/mem/items/{}/history", enc(id)), None),
        MemAction::Append { id, text, file, changed_by } => {
            let resolved = resolve_remote_text(text, file).unwrap_or_else(|| {
                eprintln!("Error: no text provided (pass as arg, --file, or pipe to stdin)");
                std::process::exit(1);
            });
            let mut payload = json!({"text": resolved});
            if let Some(v) = changed_by { payload["changed_by"] = json!(v.trim()); }
            ("POST".into(), format!("/api/mem/items/{}/append", enc(id)), Some(payload.to_string()))
        }
        MemAction::Restore { id, version, changed_by } => {
            let mut payload = serde_json::Map::new();
            if let Some(v) = version { payload.insert("version".into(), json!(v)); }
            if let Some(v) = changed_by { payload.insert("changed_by".into(), json!(v.trim())); }
            ("POST".into(), format!("/api/mem/items/{}/restore", enc(id)),
                Some(serde_json::Value::Object(payload).to_string()))
        }
        MemAction::Taxonomy { workspace } => {
            let path = match workspace {
                Some(w) => format!("/api/mem/taxonomy?workspace={}", enc(w)),
                None => "/api/mem/taxonomy".into(),
            };
            ("GET".into(), path, None)
        }
        MemAction::Tail { limit, .. } => {
            ("GET".into(), format!("/api/mem/tail?limit={}", limit), None)
        }
        MemAction::Update { id, title, summary, area, tags, body, body_file, importance, changed_by } => {
            let mut payload = serde_json::Map::new();
            if let Some(v) = title { payload.insert("title".into(), json!(v)); }
            if let Some(v) = summary { payload.insert("summary".into(), json!(v)); }
            if let Some(v) = area { payload.insert("area".into(), json!(v)); }
            if let Some(v) = tags { payload.insert("tags".into(), json!(v)); }
            if let Some(v) = resolve_remote_text(body, body_file) { payload.insert("body".into(), json!(v)); }
            if let Some(v) = importance { payload.insert("importance".into(), json!(v)); }
            if let Some(v) = changed_by {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    eprintln!("Error: --changed-by requires a non-empty actor label");
                    std::process::exit(1);
                }
                payload.insert("changed_by".into(), json!(trimmed));
            }
            ("PATCH".into(), format!("/api/mem/items/{}", enc(id)),
                Some(serde_json::Value::Object(payload).to_string()))
        }
        MemAction::Save { kind, title, body, body_file, summary, tags, area, importance, scope, workspace, pin, source_path, replace, skip_extract } => {
            let mut payload = json!({"kind": kind, "importance": importance});
            if let Some(v) = title { payload["title"] = json!(v); }
            if let Some(v) = resolve_remote_text(body, body_file) { payload["body"] = json!(v); }
            if let Some(v) = summary { payload["summary"] = json!(v); }
            if let Some(v) = tags { payload["tags"] = json!(v); }
            if let Some(v) = area { payload["area"] = json!(v); }
            let resolved_scope = match (workspace, scope) {
                (Some(ws), _) => {
                    let ws = ws.trim();
                    if ws.is_empty() {
                        eprintln!("Error: --workspace requires a non-empty id");
                        std::process::exit(1);
                    }
                    Some(format!("workspace:{ws}"))
                }
                (None, Some(s)) => Some(s.clone()),
                (None, None) => None,
            };
            if let Some(v) = resolved_scope { payload["scope"] = json!(v); }
            if let Some(v) = source_path { payload["source_path"] = json!(v); }
            if *replace { payload["replace"] = json!(true); }
            if *skip_extract { payload["skip_extract"] = json!(true); }
            if *pin { payload["pinned"] = json!(true); }
            ("POST".into(), "/api/mem/items".into(), Some(payload.to_string()))
        }
        MemAction::SavePreference { title, body, body_file, summary, tags, area, importance, scope, workspace, pin, replace } => {
            let mut payload = json!({"kind": "preference", "importance": importance, "skip_extract": true});
            if let Some(v) = title { payload["title"] = json!(v); }
            if let Some(v) = resolve_remote_text(body, body_file) { payload["body"] = json!(v); }
            if let Some(v) = summary { payload["summary"] = json!(v); }
            if let Some(v) = tags { payload["tags"] = json!(v); }
            if let Some(v) = area { payload["area"] = json!(v); }
            let resolved_scope = match (workspace, scope) {
                (Some(ws), _) => {
                    let ws = ws.trim();
                    if ws.is_empty() {
                        eprintln!("Error: --workspace requires a non-empty id");
                        std::process::exit(1);
                    }
                    Some(format!("workspace:{ws}"))
                }
                (None, Some(s)) => Some(s.clone()),
                (None, None) => None,
            };
            if let Some(v) = resolved_scope { payload["scope"] = json!(v); }
            if *replace { payload["replace"] = json!(true); }
            if *pin { payload["pinned"] = json!(true); }
            ("POST".into(), "/api/mem/items".into(), Some(payload.to_string()))
        }
        MemAction::WakeUp { budget, l0_only, no_l0, explain, workspace } => {
            let mut path = String::from("/api/mem/wake-up");
            let mut sep = '?';
            if let Some(b) = budget { path.push(sep); sep = '&'; path.push_str(&format!("budget={}", b)); }
            if *l0_only { path.push(sep); sep = '&'; path.push_str("l0_only=true"); }
            if *no_l0 { path.push(sep); sep = '&'; path.push_str("no_l0=true"); }
            if *explain { path.push(sep); sep = '&'; path.push_str("explain=true"); }
            if let Some(w) = workspace {
                path.push(sep); let _ = sep;
                path.push_str(&format!("workspace={}", enc(w)));
            }
            ("GET".into(), path, None)
        }
        MemAction::ExtractPending { limit } => {
            ("POST".into(), format!("/api/mem/extract-pending?limit={}", limit), None)
        }
        MemAction::Stats => ("GET".into(), "/api/mem/stats".into(), None),
        MemAction::Learn { text, kind, tags, error, feature } => {
            let resolved_text = text.clone().or_else(|| error.clone()).or_else(|| feature.clone()).unwrap_or_default();
            let resolved_kind = kind.clone()
                .or_else(|| if error.is_some() { Some("error".into()) } else { None })
                .or_else(|| if feature.is_some() { Some("feature".into()) } else { None })
                .unwrap_or_else(|| "insight".into());
            let mut payload = json!({"text": resolved_text, "kind": resolved_kind});
            if let Some(v) = tags { payload["tags"] = json!(v); }
            ("POST".into(), "/api/mem/learn".into(), Some(payload.to_string()))
        }
        MemAction::LearnList => {
            ("GET".into(), "/api/mem/search?kind=learning&limit=50".into(), None)
        }
        MemAction::LearnPromote { id } => {
            ("POST".into(), format!("/api/mem/promote/{}", enc(id)), None)
        }
        MemAction::Sync { path } => {
            let url = match path {
                Some(p) => format!("/api/mem/sync?path={}", enc(p)),
                None => "/api/mem/sync".into(),
            };
            ("POST".into(), url, None)
        }
        MemAction::Dream { action } => {
            use crate::MemDreamAction;
            match action {
                MemDreamAction::Run { phase } => (
                    "POST".into(),
                    format!("/api/mem/dream?phase={}", enc(phase)),
                    None,
                ),
                MemDreamAction::Apply { .. } | MemDreamAction::Diary { .. } => {
                    unsupported("mem dream apply/diary are local-only and cannot run over --mesh")
                }
            }
        }
        MemAction::Graph { action } => match action {
            MemGraphAction::Create { id, entity_type, props, scope } => {
                let props_val: serde_json::Value = serde_json::from_str(props).unwrap_or(json!({}));
                let mut payload = json!({"id": id, "type": entity_type, "properties": props_val});
                if let Some(s) = scope { payload["scope"] = json!(s); }
                ("POST".into(), "/api/mem/graph/entities".into(), Some(payload.to_string()))
            }
            MemGraphAction::Relate { from, rel, to, valid_from, valid_to, confidence, scope, reason } => {
                let mut payload = json!({"from": from, "rel": rel, "to": to, "confidence": confidence});
                if let Some(v) = valid_from { payload["valid_from"] = json!(v); }
                if let Some(v) = valid_to { payload["valid_to"] = json!(v); }
                if let Some(v) = scope { payload["scope"] = json!(v); }
                if let Some(v) = reason { payload["reason"] = json!(v); }
                ("POST".into(), "/api/mem/graph/relate".into(), Some(payload.to_string()))
            }
            MemGraphAction::Related { id, rel, as_of, include_invalidated } => {
                let mut path = format!("/api/mem/graph/related?id={}", enc(id));
                if let Some(r) = rel { path.push_str(&format!("&rel={}", enc(r))); }
                if let Some(a) = as_of { path.push_str(&format!("&as_of={}", enc(a))); }
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Invalidate { from, rel, to, ended } => {
                let mut payload = json!({"from": from, "rel": rel, "to": to});
                if let Some(e) = ended { payload["ended"] = json!(e); }
                ("POST".into(), "/api/mem/graph/invalidate".into(), Some(payload.to_string()))
            }
            MemGraphAction::Timeline { entity, include_invalidated } => {
                let mut path = format!("/api/mem/graph/timeline?entity={}", enc(entity));
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Traverse { from, hops, max_nodes, as_of, include_invalidated } => {
                let mut path = format!("/api/mem/graph/traverse?from={}&hops={}&max_nodes={}", enc(from), hops, max_nodes);
                if let Some(a) = as_of { path.push_str(&format!("&as_of={}", enc(a))); }
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Path { from, to, max_hops, include_invalidated } => {
                let mut path = format!("/api/mem/graph/path?from={}&to={}&max_hops={}", enc(from), enc(to), max_hops);
                if *include_invalidated { path.push_str("&include_invalidated=true"); }
                ("GET".into(), path, None)
            }
            MemGraphAction::Tunnels { include_invalidated } => {
                let path = if *include_invalidated {
                    "/api/mem/graph/tunnels?include_invalidated=true".to_string()
                } else {
                    "/api/mem/graph/tunnels".to_string()
                };
                ("GET".into(), path, None)
            }
            MemGraphAction::Alias { .. } => ("POST".into(), "/api/mem/graph/alias".into(), None),
            MemGraphAction::FindDuplicateRelations { group_by, limit } => {
                let group_by_vec: Vec<&str> = group_by.split(',').map(|s| s.trim()).collect();
                let payload = json!({"group_by": group_by_vec, "limit": limit});
                ("POST".into(), "/api/mem/graph/repair/find-relations".into(), Some(payload.to_string()))
            }
            MemGraphAction::DeleteRelations { rowids, actor } => {
                let rowids_vec: Vec<i64> = rowids.split(',')
                    .filter_map(|s| s.trim().parse::<i64>().ok())
                    .collect();
                let payload = json!({"rowids": rowids_vec, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/delete-relations".into(), Some(payload.to_string()))
            }
            MemGraphAction::MergeRelations { keep, merge, actor } => {
                let merge_vec: Vec<i64> = merge.split(',')
                    .filter_map(|s| s.trim().parse::<i64>().ok())
                    .collect();
                let payload = json!({"keep": keep, "merge": merge_vec, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/merge-relations".into(), Some(payload.to_string()))
            }
            MemGraphAction::FindDuplicateEntities { strategy, threshold, entity_type, limit } => {
                let strategy_value = match strategy.as_str() {
                    "alias-chain" | "alias_chain" => json!({"kind": "alias_chain"}),
                    "vector" => {
                        let mut v = json!({"kind": "vector", "threshold": threshold});
                        if let Some(et) = entity_type.clone() {
                            v["entity_type"] = serde_json::Value::String(et);
                        }
                        v
                    }
                    _ => json!({"kind": "exact_name"}),
                };
                let payload = json!({"strategy": strategy_value, "limit": limit});
                ("POST".into(), "/api/mem/graph/repair/find-entities".into(), Some(payload.to_string()))
            }
            MemGraphAction::AugmentEntity { id, patch, actor } => {
                let patch_val: serde_json::Value = serde_json::from_str(patch).unwrap_or(json!({}));
                let payload = json!({"id": id, "patch": patch_val, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/augment-entity".into(), Some(payload.to_string()))
            }
            MemGraphAction::RepairOrphanVectors { dry_run, actor } => {
                let payload = json!({"dry_run": dry_run, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/orphan-vectors".into(), Some(payload.to_string()))
            }
            MemGraphAction::PruneThinVectors { dry_run, actor } => {
                let payload = json!({"dry_run": dry_run, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/prune-thin-vectors".into(), Some(payload.to_string()))
            }
            MemGraphAction::CollapseSynonymEdges { dry_run, actor } => {
                let payload = json!({"dry_run": dry_run, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/collapse-synonym-edges".into(), Some(payload.to_string()))
            }
            MemGraphAction::PruneDreamHubEdges { dry_run, actor } => {
                let payload = json!({"dry_run": dry_run, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/prune-dream-hub-edges".into(), Some(payload.to_string()))
            }
            MemGraphAction::RepairOrphanBlobs { dry_run, actor } => {
                let payload = json!({"dry_run": dry_run, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/orphan-blobs".into(), Some(payload.to_string()))
            }
            MemGraphAction::DeleteEntity { id, force, purge_history, actor } => {
                let payload = json!({"id": id, "force": force, "purge_history": purge_history, "actor": actor});
                ("POST".into(), "/api/mem/graph/repair/delete-entity".into(), Some(payload.to_string()))
            }
            MemGraphAction::Route { .. }
            | MemGraphAction::PurgeInvalidated { .. }
            | MemGraphAction::ReviveBroken { .. }
            | MemGraphAction::IntroduceTopic { .. }
            | MemGraphAction::Hubs { .. }
            | MemGraphAction::Islands { .. }
            | MemGraphAction::Orphans { .. }
            | MemGraphAction::Get { .. } => {
                unsupported("this mem graph subcommand is local-only and cannot run over --mesh")
            }
        },
        // Local-only subcommands (PTY/subprocess/folders/keychain) never relay.
        // Return explicit errors; `Secret` is rejected upstream.
        MemAction::Flush { .. }
        | MemAction::RunWorker { .. }
        | MemAction::Skill { .. }
        | MemAction::Worker { .. }
        | MemAction::Secret { .. } => unsupported(
            "this mem subcommand is local-only (flush / run-worker / skill / worker / secret) and cannot run over --mesh",
        ),
        MemAction::Reembed { scope } => (
            "POST".into(),
            format!("/api/mem/reembed?scope={}", enc(scope)),
            None,
        ),
        MemAction::Calibration { action } => {
            use crate::MemCalibrationAction;
            match action {
                MemCalibrationAction::Log { since, limit, count } => {
                    let mut path = String::from("/api/mem/calibration/log");
                    let mut sep = '?';
                    if let Some(s) = since {
                        path.push(sep);
                        sep = '&';
                        path.push_str(&format!("since={}", enc(s)));
                    }
                    if let Some(l) = limit {
                        path.push(sep);
                        sep = '&';
                        path.push_str(&format!("limit={}", l));
                    }
                    if *count {
                        path.push(sep);
                        let _ = sep;
                        path.push_str("count=true");
                    }
                    ("GET".into(), path, None)
                }
                MemCalibrationAction::Sweep { force } => {
                    let path = if *force {
                        "/api/mem/calibration/sweep?force=true".to_string()
                    } else {
                        "/api/mem/calibration/sweep".to_string()
                    };
                    ("POST".into(), path, None)
                }
            }
        }
        MemAction::Pin { id } => (
            "PATCH".into(),
            format!("/api/mem/items/{}", enc(id)),
            Some(json!({"pinned": true}).to_string()),
        ),
        MemAction::Unpin { id } => (
            "PATCH".into(),
            format!("/api/mem/items/{}", enc(id)),
            Some(json!({"pinned": false}).to_string()),
        ),
        MemAction::Pinned { limit } => {
            let path = match limit {
                Some(n) => format!("/api/mem/pinned?limit={}", n),
                None => "/api/mem/pinned".into(),
            };
            ("GET".into(), path, None)
        }
    }
}

pub(crate) fn service_to_api(action: &ServiceAction) -> (String, String, Option<String>) {
    match action {
        ServiceAction::List { kind, enabled } => {
            let mut path = String::from("/api/services");
            let mut sep = '?';
            if let Some(k) = kind {
                path.push(sep);
                sep = '&';
                path.push_str(&format!("kind={}", urlencoding::encode(k)));
            }
            if let Some(e) = enabled {
                path.push(sep);
                let _ = sep;
                path.push_str(&format!("enabled={}", e));
            }
            ("GET".into(), path, None)
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
            let mut body = json!({
                "name": name,
                "kind": kind,
                "health_mode": health_mode,
                "health_interval": health_interval,
                "push_enabled": push,
            });
            if let Some(v) = url {
                body["url"] = json!(v);
            }
            if let Some(v) = host {
                body["host"] = json!(v);
            }
            if let Some(v) = port {
                body["port"] = json!(v);
            }
            if let Some(v) = process_label {
                body["process_label"] = json!(v);
            }
            if let Some(v) = script_path {
                body["script_path"] = json!(v);
            }
            if let Some(v) = health_path {
                body["health_path"] = json!(v);
            }
            if let Some(v) = health_json_field {
                body["health_json_field"] = json!(v);
            }
            if let Some(v) = health_ok_values {
                body["health_ok_values"] = json!(v);
            }
            if let Some(v) = health_degraded_values {
                body["health_degraded_values"] = json!(v);
            }
            if let Some(v) = description {
                body["description"] = json!(v);
            }
            (
                "POST".into(),
                "/api/services".into(),
                Some(body.to_string()),
            )
        }
        ServiceAction::Show { id } => (
            "GET".into(),
            format!("/api/services/{}", urlencoding::encode(id)),
            None,
        ),
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
            let mut body = json!({});
            if let Some(v) = name {
                body["name"] = json!(v);
            }
            if let Some(v) = url {
                body["url"] = json!(v);
            }
            if let Some(v) = health_path {
                body["health_path"] = json!(v);
            }
            if let Some(v) = health_json_field {
                body["health_json_field"] = json!(v);
            }
            if let Some(v) = health_ok_values {
                body["health_ok_values"] = json!(v);
            }
            if let Some(v) = health_degraded_values {
                body["health_degraded_values"] = json!(v);
            }
            if let Some(v) = enabled {
                body["enabled"] = json!(v);
            }
            (
                "PUT".into(),
                format!("/api/services/{}", urlencoding::encode(id)),
                Some(body.to_string()),
            )
        }
        ServiceAction::Delete { id } => (
            "DELETE".into(),
            format!("/api/services/{}", urlencoding::encode(id)),
            None,
        ),
        ServiceAction::Token { id, rotate } => {
            if *rotate {
                (
                    "POST".into(),
                    format!("/api/services/{}/regenerate-token", urlencoding::encode(id)),
                    None,
                )
            } else {
                (
                    "GET".into(),
                    format!("/api/services/{}", urlencoding::encode(id)),
                    None,
                )
            }
        }
    }
}

/// `agent spawn` carries the widest body in the mesh map; it stays here as
/// per-command REST construction, not a transport decision.
pub(super) fn agent_to_api(action: &AgentAction) -> (String, String, Option<String>) {
    match action {
        crate::AgentAction::Spawn {
            agent,
            title,
            description,
            task,
            task_file,
            role,
            parent,
            plan,
            report_to,
            workspace,
            can_spawn,
            cwd,
            args,
            model,
            reasoning_effort,
            system_prompt,
            preset,
            charter,
            charter_file,
            interval,
            nudge_cooldown,
            max_actions,
            bound_task,
            plan_task,
            team,
            execution_intent,
            provider_reserve_pct,
            non_user_facing,
        } => {
            // Team membership is authorized against the caller's own tab, which
            // has no meaning on the peer that owns the Team.
            if team.is_some() {
                return unsupported(
                    "agent spawn --team is local-only: run it on the peer that owns the Team",
                );
            }
            let task = match crate::cmd::agent::resolve_spawn_task(
                task.clone(),
                task_file.clone(),
                cwd.as_deref(),
                &mut std::io::stdin(),
            ) {
                Ok(t) => t,
                Err(e) => return unsupported(&format!("agent spawn task input error: {}", e)),
            };
            let charter = match crate::cmd::agent::resolve_spawn_charter(
                charter.clone(),
                charter_file.clone(),
                &mut std::io::stdin(),
            ) {
                Ok(t) => t,
                Err(e) => return unsupported(&format!("agent spawn charter input error: {}", e)),
            };
            let agent_name = agent.as_str();
            let title = crate::cmd::agent::join_cli_words(title.clone());
            let mut body = serde_json::json!({
                "agent": agent_name,
                "title": title,
                "task": task,
            });
            if let Some(d) = crate::cmd::agent::join_cli_words(description.clone()) {
                match crate::cmd::agent::normalize_tab_description(&d) {
                    Ok(Some(d)) => body["description"] = serde_json::json!(d),
                    Ok(None) => body["description"] = serde_json::json!(""),
                    Err(e) => return unsupported(&e),
                }
            }
            if let Some(r) = role {
                body["role"] = serde_json::json!(r.as_str());
                if let Some(profile) = r.role_profile() {
                    body["role_profile"] = serde_json::json!(profile);
                }
            }
            if let Some(p) = parent {
                body["parent_tab_id"] = serde_json::json!(p);
            }
            if let Some(pl) = plan {
                body["plan_id"] = serde_json::json!(pl);
            }
            let report_to = report_to.clone().or_else(|| {
                std::env::var("CODETERM_TAB_ID")
                    .ok()
                    .filter(|s| !s.is_empty())
            });
            if let Some(rt) = report_to {
                body["report_to"] = serde_json::json!(rt);
            }
            if let Some(w) = workspace {
                body["workspace_id"] = serde_json::json!(w);
            }
            if *can_spawn {
                body["can_spawn"] = serde_json::json!(true);
            }
            if let Some(c) = cwd {
                body["cwd"] = serde_json::json!(c);
            }
            if let Some(a) = args {
                if !a.is_empty() {
                    body["args"] = serde_json::json!(a);
                }
            }
            if let Some(m) = model {
                body["model"] = serde_json::json!(m);
            }
            if let Some(effort) = reasoning_effort {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
            if let Some(system_prompt) = system_prompt {
                body["system_prompt"] = serde_json::json!(system_prompt);
            }
            if let Some(preset) = preset {
                body["preset"] = serde_json::json!(preset);
            }
            if let Some(charter) = charter {
                body["charter"] = serde_json::json!(charter);
            }
            if let Some(interval) = interval {
                let secs = match crate::cmd::agent::parse_duration_secs(interval, "--interval") {
                    Ok(v) => v,
                    Err(e) => return unsupported(&e),
                };
                body["tick_interval_secs"] = serde_json::json!(secs);
            }
            if let Some(nudge_cooldown) = nudge_cooldown {
                let secs = match crate::cmd::agent::parse_duration_secs(
                    nudge_cooldown,
                    "--nudge-cooldown",
                ) {
                    Ok(v) => v,
                    Err(e) => return unsupported(&e),
                };
                body["nudge_cooldown_secs"] = serde_json::json!(secs);
            }
            if let Some(max_actions) = max_actions {
                body["max_actions_per_tick"] = serde_json::json!(max_actions);
            }
            if let Some(tid) = bound_task {
                body["task_id"] = serde_json::json!(tid);
            }
            if let Some(pt) = plan_task {
                body["plan_task"] = serde_json::json!(pt);
            }
            if let Some(intent) = execution_intent {
                body["execution_intent"] = serde_json::json!(intent);
                body["provider_reserve_pct"] = serde_json::json!(provider_reserve_pct);
            }
            if *non_user_facing {
                body["audience"] = serde_json::json!("automation");
            }
            (
                "POST".into(),
                "/api/agents/spawn".into(),
                Some(body.to_string()),
            )
        }
        crate::AgentAction::Providers | crate::AgentAction::Models { .. } => {
            ("GET".into(), "/api/agents/providers".into(), None)
        }
        _ => ("GET".into(), "/api/tabs".into(), None),
    }
}
