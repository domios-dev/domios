use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct ListDirQuery {
    path: Option<String>,
    #[serde(default)]
    show_hidden: bool,
}

#[derive(Serialize)]
pub(crate) struct DirEntry {
    name: String,
    path: String,
}

#[derive(Serialize)]
pub(crate) struct ListDirResponse {
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
    path.to_string_lossy().into_owned()
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

pub async fn list_dir(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListDirQuery>,
) -> Result<Json<ListDirResponse>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
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

    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&current)
        .map_err(|e| (StatusCode::FORBIDDEN, format!("Cannot read directory: {e}")))?;
    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !query.show_hidden && name.starts_with('.') {
            continue;
        }
        entries.push(DirEntry {
            name,
            path: display_path(&entry.path()),
        });
    }
    entries.sort_by_key(|e| e.name.to_lowercase());

    Ok(Json(ListDirResponse {
        path: display_path(&current),
        parent: parent_path(&current),
        home: dirs::home_dir().map(|p| display_path(&p)),
        roots: root_entries(),
        entries,
    }))
}
