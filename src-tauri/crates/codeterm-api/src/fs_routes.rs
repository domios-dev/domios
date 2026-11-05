use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::routes::AppState;

#[derive(Deserialize)]
pub struct ListDirQuery {
    path: Option<String>,
    #[serde(default)]
    show_hidden: bool,
    #[serde(default)]
    files_only: bool,
}

#[derive(Serialize)]
pub(crate) struct DirEntry {
    name: String,
    path: String,
}

#[derive(Serialize)]
pub struct ListDirResponse {
    path: String,
    parent: Option<String>,
    home: Option<String>,
    roots: Vec<DirEntry>,
    entries: Vec<DirEntry>,
}

fn expand_path(input: Option<String>) -> PathBuf {
    let raw = input.unwrap_or_default();
    if raw.is_empty() || raw == "~" {
        return dirs::home_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

fn display_path(path: &Path) -> String {
    codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy())
}

fn parent_path(path: &Path) -> Option<String> {
    path.parent().map(display_path)
}

#[cfg(target_os = "windows")]
fn root_entries() -> Vec<DirEntry> {
    let mut roots = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if Path::new(&drive).exists() {
            roots.push(DirEntry {
                name: drive.clone(),
                path: drive,
            });
        }
    }
    roots
}

#[cfg(not(target_os = "windows"))]
fn root_entries() -> Vec<DirEntry> {
    vec![DirEntry {
        name: "/".to_string(),
        path: "/".to_string(),
    }]
}

#[derive(Deserialize)]
pub struct MkdirBody {
    parent: String,
    name: String,
}

/// Create one new directory inside an existing one — the picker's
/// "New folder". Same trust model as list_dir: a console-token holder
/// can already run arbitrary shell commands.
pub async fn mkdir(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<MkdirBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || mkdir_blocking(body))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn mkdir_blocking(body: MkdirBody) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = body.name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid folder name: {name:?}"),
        ));
    }
    let parent = expand_path(Some(body.parent));
    if !parent.is_dir() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Parent is not a directory: {}", display_path(&parent)),
        ));
    }
    let target = parent.join(name);
    if target.exists() {
        return Err((StatusCode::CONFLICT, format!("Already exists: {name}")));
    }
    std::fs::create_dir(&target)
        .map_err(|e| (StatusCode::FORBIDDEN, format!("Create failed: {e}")))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "path": display_path(&target),
    })))
}

pub async fn list_dir(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Query(query): Query<ListDirQuery>,
) -> Result<Json<ListDirResponse>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || list_dir_blocking(query))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn list_dir_blocking(query: ListDirQuery) -> Result<Json<ListDirResponse>, (StatusCode, String)> {
    let requested = expand_path(query.path);
    let current = requested
        .canonicalize()
        .unwrap_or_else(|_| requested.clone());
    if !current.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Path is not a directory".to_string(),
        ));
    }

    let entries = directory_entries(&current, query.show_hidden, query.files_only)
        .map_err(|error| (StatusCode::FORBIDDEN, error))?;

    Ok(Json(ListDirResponse {
        path: display_path(&current),
        parent: parent_path(&current),
        home: dirs::home_dir().map(|p| display_path(&p)),
        roots: root_entries(),
        entries,
    }))
}

fn directory_entries(
    current: &Path,
    show_hidden: bool,
    files_only: bool,
) -> Result<Vec<DirEntry>, String> {
    let mut entries = Vec::new();
    let read_dir =
        std::fs::read_dir(&current).map_err(|e| format!("Cannot read directory: {e}"))?;
    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !(if files_only {
            file_type.is_file()
        } else {
            file_type.is_dir()
        }) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        entries.push(DirEntry {
            name,
            path: display_path(&entry.path()),
        });
    }
    entries.sort_by_key(|e| e.name.to_lowercase());

    Ok(entries)
}

#[cfg(test)]
mod tests;
