use crate::client::SocketClient;
use std::path::{Path, PathBuf};

pub fn resolve_workspace_root(
    client: &mut SocketClient,
    workspace_id: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(id) = workspace_id {
        return resolve_via_api(client, id);
    }
    if let Ok(env_id) = std::env::var("CODETERM_WORKSPACE_ID") {
        if !env_id.is_empty() {
            return resolve_via_api(client, &env_id);
        }
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if let Some(root) = walk_up_for_marker(&cwd) {
        return Ok(root);
    }
    Err("Not inside a workspace. Pass --workspace WS_ID or run from a workspace cwd.".into())
}

pub fn resolve_workspace_id(
    client: &mut SocketClient,
    workspace_id: Option<&str>,
) -> Result<String, String> {
    if let Some(id) = workspace_id {
        return Ok(id.to_string());
    }
    if let Ok(env_id) = std::env::var("CODETERM_WORKSPACE_ID") {
        if !env_id.is_empty() {
            return Ok(env_id);
        }
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let cwd_str = cwd.to_string_lossy().to_string();
    let value = client
        .call("workspace.list", None)
        .map_err(|e| format!("workspace.list failed: {e}"))?;
    let arr = value.as_array().ok_or_else(|| "workspace.list: expected array".to_string())?;
    let ws = arr
        .iter()
        .find(|w| w.get("cwd").and_then(|v| v.as_str()) == Some(cwd_str.as_str()))
        .ok_or_else(|| {
            "Not inside a workspace. Pass --workspace WS_ID or run from a workspace cwd.".to_string()
        })?;
    let id = ws
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "workspace.list entry has no id".to_string())?;
    Ok(id.to_string())
}

fn resolve_via_api(client: &mut SocketClient, id: &str) -> Result<PathBuf, String> {
    let value = client
        .call("workspace.list", None)
        .map_err(|e| format!("workspace.list failed: {e}"))?;
    let arr = value.as_array().ok_or_else(|| "workspace.list: expected array".to_string())?;
    let ws = arr
        .iter()
        .find(|w| w.get("id").and_then(|v| v.as_str()) == Some(id))
        .ok_or_else(|| format!("workspace not found: {id}"))?;
    if ws.get("name").and_then(|v| v.as_str()) == Some("General") {
        return Err("General workspace is excluded from project-map indexing.".into());
    }
    let cwd = ws
        .get("cwd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("workspace {id} has no cwd"))?;
    Ok(PathBuf::from(cwd))
}

fn walk_up_for_marker(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".codeterm").join("map").is_dir()
            || cur.join(".git").exists()
            || cur.join(".codeterm").join("workspace.json").is_file()
        {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_up_finds_git() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let found = walk_up_for_marker(&nested).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn walk_up_finds_codeterm_map() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".codeterm/map/v1")).unwrap();
        let nested = tmp.path().join("deep/dir");
        std::fs::create_dir_all(&nested).unwrap();
        let found = walk_up_for_marker(&nested).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn walk_up_returns_none_when_no_marker() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(walk_up_for_marker(tmp.path()).is_none());
    }
}
