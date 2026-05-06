pub mod bridge;
pub mod cache;
pub mod chunks;
pub mod config_io;
pub mod embed;
pub mod graph;
pub mod index;
pub mod lang;
pub mod pagerank;
pub mod scip_loader;
pub mod search;
pub mod status;
pub mod store;
pub mod tags;
pub mod walker;
pub mod workspace;

use crate::args::map::{MapAction, MapConfigAction};
use crate::client::SocketClient;
use crate::cmd::print::print_object;

pub fn handle(client: &mut SocketClient, json: bool, action: MapAction) {
    match action {
        MapAction::Index { workspace, full, verbose } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            match index::run_index(&root, index::IndexOptions { force: full, verbose }) {
                Ok(report) => {
                    let v = serde_json::to_value(&report).unwrap_or(serde_json::json!({}));
                    if json {
                        print_object(&v);
                    } else {
                        println!(
                            "indexed {} files ({} cached) — {} nodes, {} edges, {} errors in {}ms",
                            report.files_indexed,
                            report.files_cached,
                            report.nodes,
                            report.edges,
                            report.errors,
                            report.elapsed_ms
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        MapAction::Status { workspace } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            match status::run_status(&root) {
                Ok(status) => {
                    if json {
                        let v = serde_json::to_value(&status).unwrap_or(serde_json::json!({}));
                        print_object(&v);
                    } else {
                        println!(
                            "{}\n  schema v{}, indexer v{}\n  files={}, nodes={}, edges={}, errors={}\n  last index: {} → {}",
                            status.db_path,
                            status.schema_version.as_deref().unwrap_or("?"),
                            status.indexer_version.as_deref().unwrap_or("?"),
                            status.files,
                            status.nodes,
                            status.edges,
                            status.errors,
                            status.last_index_started_at.as_deref().unwrap_or("never"),
                            status.last_index_finished_at.as_deref().unwrap_or("never"),
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        MapAction::Config { action } => {
            handle_config(client, json, action);
        }
        MapAction::Export { workspace, workspace_root, node, k_hops, node_cap } => {
            let root = if let Some(path) = workspace_root {
                std::path::PathBuf::from(path)
            } else {
                match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            };
            let q = match graph::GraphQuery::open(&root) {
                Ok(q) => q,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let result = match node {
                Some(id) => q.focus_subgraph(id, k_hops),
                None => q.full_graph(node_cap),
            };
            match result {
                Ok(payload) => {
                    let v = serde_json::to_value(&payload).unwrap_or(serde_json::json!({}));
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        MapAction::Show { workspace, node: _ } => {
            let ws_id = match workspace::resolve_workspace_id(client, workspace.as_deref()) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let (base, token) = crate::read_api_config();
            let http = crate::build_http(&token);
            match http
                .post(format!("{}/api/map/show", base))
                .json(&serde_json::json!({ "workspace_id": ws_id }))
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if !status.is_success() {
                        eprintln!("Error: {}", status);
                        if let Some(s) = val.as_str() { eprintln!("{}", s); }
                        std::process::exit(1);
                    }
                    if json {
                        print_object(&val);
                    } else if let Some(pid) = val.get("pane_id").and_then(|v| v.as_str()) {
                        println!("opened project map pane {}", pid);
                    } else {
                        println!("opened project map");
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        MapAction::Search { query, workspace, limit, kind, semantic_only, vec_k } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let opts = search::SearchOptions {
                query: &query,
                kind: kind.as_deref(),
                limit,
                semantic_only,
                vec_k,
            };
            match search::run_search(&root, opts) {
                Ok(rows) => {
                    if json {
                        let v = serde_json::to_value(&rows).unwrap_or(serde_json::json!([]));
                        print_object(&v);
                    } else if rows.is_empty() {
                        println!("no matches");
                    } else {
                        for r in rows {
                            let path = r.path.unwrap_or_else(|| "-".to_string());
                            let line = r
                                .start_line
                                .map(|l| format!(":{}", l + 1))
                                .unwrap_or_default();
                            println!("[{}] {} — {}{}", r.kind, r.display_name, path, line);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn handle_config(client: &mut SocketClient, json: bool, action: MapConfigAction) {
    match action {
        MapConfigAction::Get { workspace, key } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let cfg = config_io::read_config(&root);
            match key {
                Some(k) => match config_io::get_value(&cfg, &k) {
                    Some(v) => {
                        if json {
                            print_object(&serde_json::json!({ k: v }));
                        } else {
                            println!("{}", value_to_human(&v));
                        }
                    }
                    None => {
                        eprintln!("Error: unknown key: {k}");
                        std::process::exit(1);
                    }
                },
                None => {
                    let v = serde_json::to_value(&cfg).unwrap_or(serde_json::json!({}));
                    if json {
                        print_object(&v);
                    } else {
                        for (k, val) in v.as_object().cloned().unwrap_or_default() {
                            println!("  {} = {}", k, value_to_human(&val));
                        }
                    }
                }
            }
        }
        MapConfigAction::Set { key, value, workspace } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let mut cfg = config_io::read_config(&root);
            if let Err(e) = config_io::set_value(&mut cfg, &key, &value) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
            if let Err(e) = config_io::write_config(&root, &cfg) {
                eprintln!("Error: write config: {e}");
                std::process::exit(1);
            }
            if json {
                let v = serde_json::to_value(&cfg).unwrap_or(serde_json::json!({}));
                print_object(&v);
            } else {
                println!("set {} = {}", key, value);
            }
        }
    }
}

fn value_to_human(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "(none)".to_string(),
        other => other.to_string(),
    }
}

