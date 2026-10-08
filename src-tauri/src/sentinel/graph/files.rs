use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::sentinel::store::Sentinel;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct FileReference {
    pub name: String,
    pub path: String,
    pub short_path: String,
    pub exists: bool,
}

#[derive(Clone, Copy)]
pub enum FileKind {
    File,
    Missing,
    Other,
}

fn literals(source: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut value = String::new();
    let mut quote = None;
    for character in source.chars() {
        if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if quote.is_none()
            && (character.is_whitespace() || ",;|&><(){}[]".contains(character))
        {
            if !value.is_empty() {
                values.push(std::mem::take(&mut value));
            }
        } else {
            value.push(character);
        }
    }
    if quote.is_none() && !value.is_empty() {
        values.push(value);
    }
    values
}

fn candidate(value: &str) -> Option<&str> {
    let value = if value.starts_with('-') {
        value.split_once('=')?.1
    } else {
        value
    };
    if value.is_empty() || value.contains("://") || value.chars().any(|c| "$%*?`=\n\r".contains(c))
    {
        return None;
    }
    let path = Path::new(value);
    (path.is_absolute() || value.contains(['/', '\\']) || path.extension().is_some())
        .then_some(value)
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => result.push(".."),
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub fn references(
    source: &str,
    cwd: &Path,
    inspect: impl Fn(&Path) -> FileKind,
) -> Vec<FileReference> {
    let mut seen = BTreeSet::new();
    literals(source)
        .iter()
        .filter_map(|literal| {
            let candidate = candidate(literal)?;
            let path = normalize(&cwd.join(candidate));
            if !seen.insert(path.clone()) {
                return None;
            }
            let exists = match inspect(&path) {
                FileKind::File => true,
                FileKind::Missing => false,
                FileKind::Other => return None,
            };
            Some(FileReference {
                name: path.file_name()?.to_string_lossy().into_owned(),
                short_path: path
                    .strip_prefix(cwd)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
                path: path.to_string_lossy().into_owned(),
                exists,
            })
        })
        .take(64)
        .collect()
}

pub fn discover(
    job: &Sentinel,
    bucket: Option<&str>,
    handler: Option<&str>,
) -> Result<Vec<FileReference>, String> {
    let definition = super::detail::definition(job);
    let source = match (bucket, handler) {
        (None, None) => definition["probe"].as_str().unwrap_or_default(),
        (Some(bucket), Some(handler)) => definition["commands"]
            .as_array()
            .and_then(|commands| {
                commands
                    .iter()
                    .find(|command| command["bucket"] == bucket && command["handler_id"] == handler)
            })
            .and_then(|command| command["command"].as_str())
            .ok_or("node command definition unavailable")?,
        _ => return Err("file discovery requires an exact node or command route".into()),
    };
    let cwd = Path::new(&job.cwd);
    if !cwd.is_absolute() {
        return Err("node working directory is unavailable".into());
    }
    Ok(references(source, cwd, |path| match path.metadata() {
        Ok(metadata) if metadata.is_file() => FileKind::File,
        Ok(_) => FileKind::Other,
        Err(_) => FileKind::Missing,
    }))
}

#[cfg(test)]
mod tests;
