use std::path::{Component, Path, PathBuf};

pub fn canonical_root(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("project root must be absolute without traversal".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("project root must be an existing directory, not a symlink".into());
    }
    std::fs::canonicalize(path).map_err(|e| e.to_string())
}

pub fn canonical_target(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let mut current = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        if std::fs::symlink_metadata(&current).is_ok() {
            let mut canonical = std::fs::canonicalize(&current).ok()?;
            for part in tail.into_iter().rev() {
                canonical.push(part);
            }
            return Some(canonical);
        }
        tail.push(current.file_name()?.to_os_string());
        if !current.pop() {
            return None;
        }
    }
}

pub fn configured(
    settings: &serde_json::Value,
    pointers: &[String],
) -> Result<Vec<PathBuf>, String> {
    fn visit(
        value: &serde_json::Value,
        parts: &[&str],
        roots: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if parts.is_empty() {
            if let Some(path) = value.as_str().filter(|s| !s.is_empty()) {
                let canonical = canonical_root(path)?;
                if !roots.contains(&canonical) {
                    roots.push(canonical);
                }
            }
            return Ok(());
        }
        if parts[0] == "*" {
            if let Some(values) = value.as_array() {
                for value in values {
                    visit(value, &parts[1..], roots)?;
                }
            }
        } else if let Some(value) = value.get(parts[0]) {
            visit(value, &parts[1..], roots)?;
        }
        Ok(())
    }
    if pointers.len() > 16 {
        return Err("too many project root selectors".into());
    }
    let mut roots = Vec::new();
    for pointer in pointers {
        if !pointer.starts_with('/') || pointer.len() > 256 {
            return Err("invalid project root selector".into());
        }
        visit(
            settings,
            &pointer[1..].split('/').collect::<Vec<_>>(),
            &mut roots,
        )?;
    }
    if roots.len() > 32 {
        return Err("too many project roots".into());
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_roots_are_exact_canonical_directories() {
        let root = tempfile::tempdir().unwrap();
        let settings = serde_json::json!({"profiles":[{"root":root.path()}]});
        assert_eq!(
            configured(&settings, &["/profiles/*/root".into()]).unwrap(),
            [root.path().canonicalize().unwrap()]
        );
        assert!(canonical_target(&root.path().join("../outside")).is_none());
        assert_eq!(
            canonical_target(&root.path().join("new/nested.txt")).unwrap(),
            root.path().canonicalize().unwrap().join("new/nested.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_descendants_of_symlink_use_the_real_parent() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        assert!(!canonical_target(&root.path().join("link/missing/file"))
            .unwrap()
            .starts_with(root.path().canonicalize().unwrap()));
        assert!(canonical_root(root.path().join("link").to_str().unwrap()).is_err());
    }
}
