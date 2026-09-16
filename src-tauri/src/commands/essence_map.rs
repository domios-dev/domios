use serde::Serialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

const MAP_FILE_LIMIT: u64 = 2 * 1024 * 1024;
const SOURCE_FILE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEssenceMapFile {
    pub project_path: String,
    pub map_path: String,
    pub status: &'static str,
    pub content: Option<String>,
    pub error: Option<String>,
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn unavailable(project_path: String, map_path: String, error: String) -> WorkspaceEssenceMapFile {
    WorkspaceEssenceMapFile {
        project_path,
        map_path,
        status: "unreadable",
        content: None,
        error: Some(error),
    }
}

fn read_map(root: PathBuf) -> WorkspaceEssenceMapFile {
    let project_path = root.canonicalize().unwrap_or_else(|_| root.clone());
    let map_path = project_path.join("essences").join("map.jsonl");
    let project_display = display_path(&project_path);
    let map_display = display_path(&map_path);

    if !project_path.is_dir() {
        return unavailable(
            project_display,
            map_display,
            "Project tree is not a directory".into(),
        );
    }

    let metadata = match map_path.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return unavailable(
                project_display,
                map_display,
                "essences/map.jsonl is not a file".into(),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkspaceEssenceMapFile {
                project_path: project_display,
                map_path: map_display,
                status: "missing",
                content: None,
                error: None,
            }
        }
        Err(error) => return unavailable(project_display, map_display, error.to_string()),
    };

    if metadata.len() > MAP_FILE_LIMIT {
        return WorkspaceEssenceMapFile {
            project_path: project_display,
            map_path: map_display,
            status: "oversized",
            content: None,
            error: Some(format!("Map exceeds {MAP_FILE_LIMIT} bytes")),
        };
    }

    match std::fs::read_to_string(&map_path) {
        Ok(content) => WorkspaceEssenceMapFile {
            project_path: project_display,
            map_path: map_display,
            status: if content.trim().is_empty() {
                "empty"
            } else {
                "ready"
            },
            content: Some(content),
            error: None,
        },
        Err(error) => unavailable(project_display, map_display, error.to_string()),
    }
}

pub(crate) fn read_workspace_essence_maps_core(
    project_paths: &[String],
) -> Vec<WorkspaceEssenceMapFile> {
    let mut seen = HashSet::new();
    project_paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .filter(|path| seen.insert(path.replace('\\', "/")))
        .map(|path| read_map(PathBuf::from(path)))
        .collect()
}

pub(crate) fn read_workspace_essence_source_core(
    project_path: &str,
    source_path: &str,
) -> Result<String, String> {
    let root = Path::new(project_path.trim())
        .canonicalize()
        .map_err(|error| format!("Unable to resolve project tree: {error}"))?;
    if !root.is_dir() {
        return Err("Project tree is not a directory".into());
    }
    let relative = Path::new(source_path.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Essence source path must be relative to the project tree".into());
    }
    let source = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("Unable to resolve essence source: {error}"))?;
    if !source.starts_with(&root) || !source.is_file() {
        return Err("Essence source is outside the project tree".into());
    }
    let metadata = source
        .metadata()
        .map_err(|error| format!("Unable to inspect essence source: {error}"))?;
    if metadata.len() > SOURCE_FILE_LIMIT {
        return Err(format!("Essence source exceeds {SOURCE_FILE_LIMIT} bytes"));
    }
    std::fs::read_to_string(&source)
        .map_err(|error| format!("Unable to read essence source: {error}"))
}

#[tauri::command]
pub async fn read_workspace_essence_maps(
    project_paths: Vec<String>,
) -> Result<Vec<WorkspaceEssenceMapFile>, String> {
    tokio::task::spawn_blocking(move || Ok(read_workspace_essence_maps_core(&project_paths)))
        .await
        .map_err(|error| format!("Essence map read task failed: {error}"))?
}

#[tauri::command]
pub async fn read_workspace_essence_source(
    project_path: String,
    source_path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        read_workspace_essence_source_core(&project_path, &source_path)
    })
    .await
    .map_err(|error| format!("Essence source read task failed: {error}"))?
}
