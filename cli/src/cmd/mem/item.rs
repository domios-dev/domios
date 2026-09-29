use std::io::Read;

use crate::client::SocketClient;

/// Resolve free-text content from (inline value, file path, `-`=stdin).
/// `--file` wins; otherwise a literal `-` reads stdin; otherwise the value
/// is returned verbatim. Returns None when nothing was supplied.
pub(super) fn resolve_text(
    value: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

pub(super) fn read_path_or_stdin(path: &str) -> Result<String, String> {
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

pub(super) struct SaveMemItem {
    pub kind: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub body_file: Option<String>,
    pub summary: Option<String>,
    pub tags: Option<String>,
    pub area: Option<String>,
    pub importance: f64,
    pub scope: Option<String>,
    pub workspace: Option<String>,
    pub pin: bool,
    pub source_path: Option<String>,
    pub replace: bool,
    pub skip_extract: bool,
}

/// Resolve a `mem save` / `mem save-preference` invocation into the JSON
/// payload the daemon's `mem.save` socket method expects. Exits the process
/// on unresolvable input (missing body), matching prior CLI behavior.
pub(super) fn build_save_payload(item: SaveMemItem) -> serde_json::Value {
    if validate_save_importance(item.importance).is_err() {
        eprintln!("Error: importance must be finite and within 0.0..=5.0");
        std::process::exit(1);
    }
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
    serde_json::json!({
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
    })
}

fn validate_save_importance(importance: f64) -> Result<(), ()> {
    if importance.is_finite() && (0.0..=5.0).contains(&importance) {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::validate_save_importance;

    #[test]
    fn save_rejects_nan_and_out_of_range_importance() {
        assert!(validate_save_importance(f64::NAN).is_err());
        assert!(validate_save_importance(5.1).is_err());
    }
}

pub(crate) fn validate_mem_forget_confirmation(area: &str, yes: bool) -> Result<(), String> {
    if !yes {
        return Err(format!(
            "refusing to batch-delete area '{area}' without --yes; retry: codeterm mem forget {area} --yes"
        ));
    }
    Ok(())
}

pub(super) fn split_csv(raw: Option<String>) -> Vec<String> {
    raw.map(|tags| {
        tags.split(',')
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

pub(super) fn resolve_scope(workspace: Option<String>, scope: Option<String>) -> Option<String> {
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

pub(super) fn resolve_wakeup_workspace(
    explicit: Option<String>,
    inherited: Option<String>,
) -> Option<String> {
    explicit
        .or(inherited)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn handle_mem_history(
    client: &mut SocketClient,
    id: String,
    json_mode: bool,
) -> Result<Option<serde_json::Value>, String> {
    let val = client.call("mem.history", Some(serde_json::json!({ "id": id })));
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
    client: &mut SocketClient,
    workspace: Option<String>,
    json_mode: bool,
) -> Result<Option<serde_json::Value>, String> {
    let val = client.call(
        "mem.taxonomy",
        Some(serde_json::json!({ "workspace": workspace })),
    );
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
    use super::{resolve_text, validate_mem_forget_confirmation};

    #[test]
    fn mem_forget_refused_without_yes() {
        let err = validate_mem_forget_confirmation("auth", false).unwrap_err();
        assert!(err.contains("refusing to batch-delete area 'auth'"));
        assert!(err.contains("--yes"));
    }

    #[test]
    fn mem_forget_allowed_with_yes() {
        assert!(validate_mem_forget_confirmation("auth", true).is_ok());
    }

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
