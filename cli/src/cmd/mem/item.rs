use std::io::Read;

use super::{mem_get_request, mem_post_request};
use crate::MemAction;

/// Resolve free-text content from (inline value, file path, `-`=stdin).
/// `--file` wins; otherwise a literal `-` reads stdin; otherwise the value
/// is returned verbatim. Returns None when nothing was supplied.
fn resolve_text(value: Option<String>, file: Option<String>) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

fn read_path_or_stdin(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {}", e))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))
    }
}

struct SaveMemItem {
    kind: String,
    title: Option<String>,
    body: Option<String>,
    body_file: Option<String>,
    summary: Option<String>,
    tags: Option<String>,
    area: Option<String>,
    importance: f64,
    scope: Option<String>,
    workspace: Option<String>,
    pin: bool,
    source_path: Option<String>,
    replace: bool,
    skip_extract: bool,
}

fn save_mem_item(
    http: &reqwest::blocking::Client,
    base: &str,
    item: SaveMemItem,
) -> Result<serde_json::Value, String> {
    let resolved_body = match resolve_text(item.body, item.body_file) {
        Ok(Some(b)) => b,
        Ok(None) => {
            let mut s = String::new();
            if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                eprintln!("Error: no body provided (pass as arg or pipe to stdin)");
                std::process::exit(1);
            }
            s
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let resolved_title = item.title.unwrap_or_else(|| {
        resolved_body
            .lines()
            .next()
            .unwrap_or("untitled")
            .chars()
            .take(80)
            .collect()
    });
    let tag_list = split_csv(item.tags);
    let resolved_scope = resolve_scope(item.workspace, item.scope);
    let payload = serde_json::json!({
        "kind": item.kind,
        "title": resolved_title,
        "summary": item.summary,
        "area": item.area,
        "tags": tag_list,
        "body": resolved_body,
        "importance": item.importance,
        "scope": resolved_scope,
        "source_path": item.source_path,
        "replace": item.replace,
        "skip_extract": item.skip_extract,
        "pinned": item.pin,
    });
    mem_post_request(http, &format!("{}/api/mem/items", base), &payload)
}

fn split_csv(raw: Option<String>) -> Vec<String> {
    raw.map(|tags| {
        tags.split(',')
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn resolve_scope(workspace: Option<String>, scope: Option<String>) -> Option<String> {
    match (workspace, scope) {
        (Some(ws), _) => {
            let ws = ws.trim();
            if ws.is_empty() {
                eprintln!("Error: --workspace requires a non-empty id");
                std::process::exit(1);
            }
            Some(format!("workspace:{ws}"))
        }
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

pub(crate) fn handle_mem_items(
    http: &reqwest::blocking::Client,
    base: &str,
    action: MemAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemAction::Search {
            query,
            kind,
            area,
            scope,
            project,
            limit,
            rerank,
        } => {
            let mut url = format!(
                "{}/api/mem/search?q={}&limit={}",
                base,
                urlencoding::encode(&query),
                limit
            );
            if let Some(k) = &kind {
                url.push_str(&format!("&kind={}", k));
            }
            if let Some(a) = &area {
                url.push_str(&format!("&area={}", a));
            }
            if let Some(s) = &scope {
                url.push_str(&format!("&scope={}", s));
            }
            let project = project.or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string())
            });
            if let Some(p) = &project {
                url.push_str(&format!("&project={}", urlencoding::encode(p)));
            }
            if rerank {
                url.push_str("&rerank=true");
            }
            mem_get_request(http, &url)
        }
        MemAction::Get { id } => mem_get_request(http, &format!("{}/api/mem/items/{}", base, id)),
        MemAction::Delete { id } => {
            let resp = http
                .delete(format!("{}/api/mem/items/{}", base, id))
                .send()
                .map_err(|e| e.to_string());
            resp.map(|r| serde_json::json!({"deleted": r.status().is_success()}))
        }
        MemAction::Forget { area } => {
            let resp = http
                .delete(format!(
                    "{}/api/mem/forget/{}",
                    base,
                    urlencoding::encode(&area)
                ))
                .send()
                .map_err(|e| e.to_string());
            resp.map(|r| {
                if let Ok(v) = r.json::<serde_json::Value>() {
                    v
                } else {
                    serde_json::json!({"forgotten": area})
                }
            })
        }
        MemAction::Update {
            id,
            title,
            summary,
            area,
            tags,
            body,
            body_file,
            importance,
            changed_by,
        } => {
            let body = match resolve_text(body, body_file) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let tag_list: Option<Vec<String>> = tags.map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });
            let payload = serde_json::json!({
                "title": title, "summary": summary, "area": area,
                "tags": tag_list, "body": body, "importance": importance,
            });
            let mut req = http
                .patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload);
            if let Some(actor) = &changed_by {
                let trimmed = actor.trim();
                if trimmed.is_empty() {
                    eprintln!("Error: --changed-by requires a non-empty actor label");
                    std::process::exit(1);
                }
                req = req.header("X-CodeTerm-Actor", trimmed);
            }
            let resp = req.send().map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() {
                        Ok(val)
                    } else {
                        Err(val.to_string())
                    }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Pin { id } => {
            let payload = serde_json::json!({ "pinned": true });
            let resp = http
                .patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload)
                .send()
                .map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() {
                        Ok(val)
                    } else {
                        Err(val.to_string())
                    }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Unpin { id } => {
            let payload = serde_json::json!({ "pinned": false });
            let resp = http
                .patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload)
                .send()
                .map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() {
                        Ok(val)
                    } else {
                        Err(val.to_string())
                    }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Pinned { limit } => {
            let url = match limit {
                Some(n) => format!("{}/api/mem/pinned?limit={}", base, n),
                None => format!("{}/api/mem/pinned", base),
            };
            mem_get_request(http, &url)
        }
        MemAction::Save {
            kind,
            title,
            body,
            body_file,
            summary,
            tags,
            area,
            importance,
            scope,
            workspace,
            pin,
            source_path,
            replace,
            skip_extract,
        } => save_mem_item(
            http,
            base,
            SaveMemItem {
                kind,
                title,
                body,
                body_file,
                summary,
                tags,
                area,
                importance,
                scope,
                workspace,
                pin,
                source_path,
                replace,
                skip_extract,
            },
        ),
        MemAction::SavePreference {
            title,
            body,
            body_file,
            summary,
            tags,
            area,
            importance,
            scope,
            workspace,
            pin,
            replace,
        } => save_mem_item(
            http,
            base,
            SaveMemItem {
                kind: "preference".to_string(),
                title,
                body,
                body_file,
                summary,
                tags,
                area,
                importance,
                scope,
                workspace,
                pin,
                source_path: None,
                replace,
                skip_extract: true,
            },
        ),
        MemAction::ExtractPending { limit } => mem_post_request(
            http,
            &format!("{}/api/mem/extract-pending?limit={}", base, limit),
            &serde_json::json!({}),
        ),
        MemAction::WakeUp {
            budget,
            l0_only,
            no_l0,
            explain,
            workspace,
        } => {
            let mut url = format!("{}/api/mem/wake-up", base);
            let mut sep = '?';
            if let Some(b) = budget {
                url.push_str(&format!("{}budget={}", sep, b));
                sep = '&';
            }
            if l0_only {
                url.push_str(&format!("{}l0_only=true", sep));
                sep = '&';
            }
            if no_l0 {
                url.push_str(&format!("{}no_l0=true", sep));
                sep = '&';
            }
            if explain {
                url.push_str(&format!("{}explain=true", sep));
                sep = '&';
            }
            if let Some(ws) = &workspace {
                url.push_str(&format!("{}workspace={}", sep, urlencoding::encode(ws)));
                let _ = sep;
            }
            mem_get_request(http, &url)
        }
        MemAction::Stats => mem_get_request(http, &format!("{}/api/mem/stats", base)),
        MemAction::Learn {
            text,
            kind,
            tags,
            error,
            feature,
        } => {
            let (resolved_text, resolved_kind) = if let Some(t) = error {
                (t, "error".to_string())
            } else if let Some(t) = feature {
                (t, "feature".to_string())
            } else if let Some(t) = text {
                (t, kind.unwrap_or_else(|| "insight".into()))
            } else {
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                    eprintln!("Error: provide text as arg, --error/--feature, or pipe to stdin");
                    std::process::exit(1);
                }
                (s, kind.unwrap_or_else(|| "insight".into()))
            };
            let tag_list: Vec<String> = tags
                .map(|t| {
                    t.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            mem_post_request(
                http,
                &format!("{}/api/mem/learn", base),
                &serde_json::json!({
                    "text": resolved_text, "kind": resolved_kind, "tags": tag_list,
                }),
            )
        }
        MemAction::LearnList => mem_get_request(
            http,
            &format!("{}/api/mem/search?q=learning&kind=learning&limit=50", base),
        ),
        MemAction::LearnPromote { id } => mem_post_request(
            http,
            &format!("{}/api/mem/promote/{}", base, id),
            &serde_json::json!({}),
        ),
        MemAction::Flush { pane, title } => {
            let pane_id = pane
                .or_else(|| std::env::var("CODETERM_PANE_ID").ok())
                .unwrap_or_else(|| "current".to_string());
            mem_post_request(
                http,
                &format!("{}/api/mem/flush", base),
                &serde_json::json!({
                    "pane": pane_id, "title": title,
                }),
            )
        }
        MemAction::Sync { path } => {
            let mut url = format!("{}/api/mem/sync", base);
            if let Some(p) = path {
                url.push_str(&format!("?path={}", urlencoding::encode(&p)));
            }
            mem_post_request(http, &url, &serde_json::json!({}))
        }
        MemAction::Append {
            id,
            text,
            file,
            changed_by,
        } => {
            let resolved = match resolve_text(text, file) {
                Ok(Some(t)) => t,
                Ok(None) => match read_path_or_stdin("-") {
                    Ok(s) if !s.trim().is_empty() => s,
                    _ => {
                        eprintln!(
                            "Error: no text provided (pass as arg, --file, or pipe to stdin)"
                        );
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = serde_json::json!({ "text": resolved, "changed_by": changed_by });
            mem_post_request(
                http,
                &format!("{}/api/mem/items/{}/append", base, id),
                &payload,
            )
        }
        MemAction::Restore {
            id,
            version,
            changed_by,
        } => {
            let payload = serde_json::json!({ "version": version, "changed_by": changed_by });
            mem_post_request(
                http,
                &format!("{}/api/mem/items/{}/restore", base, id),
                &payload,
            )
        }
        _ => unreachable!("invalid item action passed to handle_mem_items"),
    }
}

pub(crate) fn handle_mem_history(
    http: &reqwest::blocking::Client,
    base: &str,
    id: String,
    json_mode: bool,
) -> Result<Option<serde_json::Value>, String> {
    let val = mem_get_request(http, &format!("{}/api/mem/items/{}/history", base, id));
    if !json_mode {
        if let Ok(v) = &val {
            if let Some(rows) = v.get("history").and_then(|h| h.as_array()) {
                crate::cmd::print::print_history_timeline(rows);
                return Ok(None);
            }
        }
    }
    Ok(Some(val?))
}

pub(crate) fn handle_mem_taxonomy(
    http: &reqwest::blocking::Client,
    base: &str,
    workspace: Option<String>,
    json_mode: bool,
) -> Result<Option<serde_json::Value>, String> {
    let mut url = format!("{}/api/mem/taxonomy", base);
    if let Some(ws) = &workspace {
        url.push_str(&format!("?workspace={}", urlencoding::encode(ws)));
    }
    let val = mem_get_request(http, &url);
    if !json_mode {
        if let Ok(ref v) = val {
            crate::cmd::print::print_taxonomy(v);
            return Ok(None);
        }
    }
    Ok(Some(val?))
}

#[cfg(test)]
mod tests {
    use super::resolve_text;

    #[test]
    fn resolve_text_returns_verbatim_value() {
        let out = resolve_text(Some("---\nname: x".into()), None).unwrap();
        assert_eq!(out.as_deref(), Some("---\nname: x"));
    }

    #[test]
    fn resolve_text_none_is_none() {
        assert!(resolve_text(None, None).unwrap().is_none());
    }

    #[test]
    fn resolve_text_reads_file() {
        let path = std::env::temp_dir().join(format!("memtest-{}-a.md", std::process::id()));
        std::fs::write(&path, "---\nfrom file\nbody").unwrap();
        let out = resolve_text(None, Some(path.to_string_lossy().to_string())).unwrap();
        assert_eq!(out.as_deref(), Some("---\nfrom file\nbody"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolve_text_file_wins_over_value() {
        let path = std::env::temp_dir().join(format!("memtest-{}-b.md", std::process::id()));
        std::fs::write(&path, "file body").unwrap();
        let out = resolve_text(
            Some("inline".into()),
            Some(path.to_string_lossy().to_string()),
        )
        .unwrap();
        assert_eq!(out.as_deref(), Some("file body"));
        let _ = std::fs::remove_file(&path);
    }
}
