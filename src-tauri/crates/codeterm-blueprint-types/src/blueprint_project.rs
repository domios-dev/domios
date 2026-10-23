use crate::blueprint_document::{self, Document};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const DIRECTORY: &str = ".domios/blueprints";
const MAX_FILES: usize = 128;
const MAX_BYTES: u64 = 65_536;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub path: String,
    pub name: Option<String>,
    pub intent: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub path: String,
    pub document: Document,
}
fn directory(root: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for part in [".domios", "blueprints"] {
        path.push(part);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(
                "Blueprint directory must be a real directory inside the repository".into(),
            );
        }
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(&root) {
        return Err("Blueprint directory escapes the repository".into());
    }
    Ok(canonical)
}
fn extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("yaml" | "yml" | "json")
    )
}
pub fn list(root: &Path) -> Result<Vec<Entry>, String> {
    if !root.join(".domios").exists() || !root.join(DIRECTORY).exists() {
        // Broken directory links must refuse even though their targets do not exist.
        for path in [root.join(".domios"), root.join(DIRECTORY)] {
            if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("Linked Blueprint directories are unavailable".into());
            }
        }
        return Ok(Vec::new());
    }
    let directory = directory(root)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if !extension(&path) {
            continue;
        }
        if paths.len() >= MAX_FILES {
            return Err("Blueprint listing exceeds 128 files".into());
        }
        paths.push(path);
    }
    paths.sort();
    let mut counts = BTreeMap::new();
    for path in &paths {
        *counts
            .entry(path.file_stem().map(|s| s.to_os_string()))
            .or_insert(0) += 1;
    }
    Ok(paths
        .into_iter()
        .map(|path| {
            let source = format!(
                "{DIRECTORY}/{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            let read = if counts[&path.file_stem().map(|s| s.to_os_string())] > 1 {
                Err("Ambiguous Blueprint stem across file extensions".into())
            } else {
                show(root, &source)
            };
            match read {
                Ok(view) => Entry {
                    path: source,
                    name: Some(view.document.name),
                    intent: view.document.intent,
                    error: None,
                },
                Err(error) => Entry {
                    path: source,
                    name: None,
                    intent: None,
                    error: Some(error),
                },
            }
        })
        .collect())
}
pub fn show(root: &Path, source: &str) -> Result<View, String> {
    let relative = Path::new(source);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.parent() != Some(Path::new(DIRECTORY))
        || !extension(relative)
    {
        return Err("Choose one file directly under .domios/blueprints/".into());
    }
    let directory = directory(root)?;
    let path = directory.join(relative.file_name().ok_or("Blueprint filename missing")?);
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("Blueprint source must be a regular unlinked file of at most 64 KiB".into());
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    if canonical.parent() != Some(directory.as_path()) {
        return Err("Blueprint source escapes its directory".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&canonical)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Blueprint source exceeds 64 KiB".into());
    }
    if path.canonicalize().map_err(|e| e.to_string())? != canonical
        || std::fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("Blueprint source changed during read".into());
    }
    let raw = String::from_utf8(bytes).map_err(|_| "Blueprint source is not UTF-8")?;
    let document = blueprint_document::parse(source, &raw)
        .map_err(|e| format!("{e:?}"))?
        .document;
    if relative.file_stem().and_then(|s| s.to_str()) != Some(document.name.as_str()) {
        return Err("Blueprint name must exactly match the file stem".into());
    }
    let matches = std::fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            extension(&entry.path()) && entry.path().file_stem() == relative.file_stem()
        })
        .take(2)
        .count();
    if matches != 1 {
        return Err("Ambiguous Blueprint stem across file extensions".into());
    }
    Ok(View {
        path: source.into(),
        document,
    })
}
