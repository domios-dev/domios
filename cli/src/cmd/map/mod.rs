pub mod cache;
pub mod index;
pub mod lang;
pub mod search;
pub mod status;
pub mod store;
pub mod tags;
pub mod walker;
pub mod workspace;

use crate::args::map::MapAction;
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
        MapAction::Search { query, workspace, limit, kind } => {
            let root = match workspace::resolve_workspace_root(client, workspace.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            match search::run_search(&root, &query, kind.as_deref(), limit) {
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
