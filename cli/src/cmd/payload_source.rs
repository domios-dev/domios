use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn current_pad() -> Result<PathBuf, String> {
    let tab_id = std::env::var("CODETERM_TAB_ID")
        .ok().filter(|v| !v.trim().is_empty())
        .ok_or("AgentPad requires CODETERM_TAB_ID; focused-tab fallback is not allowed")?;
    let configured = std::env::var_os("CODETERM_PAD_DIR")
        .ok_or("AgentPad is unavailable: CODETERM_PAD_DIR is not set")?;
    let path = PathBuf::from(configured);
    if !path.is_absolute() || path.file_name().and_then(|v| v.to_str()) != Some(tab_id.as_str()) {
        return Err("CODETERM_PAD_DIR does not belong to the current full tab id".into());
    }
    let canonical = path.canonicalize().map_err(|e| format!("resolve current AgentPad: {e}"))?;
    if !canonical.is_dir() { return Err("current AgentPad is not a directory".into()); }
    Ok(canonical)
}

pub fn read_from_pad(relative: &str) -> Result<String, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty() || relative.is_absolute()
        || relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("--from-pad must be a non-empty relative file path without traversal".into());
    }
    let root = current_pad()?;
    let file = root.join(relative).canonicalize()
        .map_err(|e| format!("resolve --from-pad {}: {e}", relative.display()))?;
    if file == root || !file.starts_with(&root) {
        return Err("--from-pad path escapes the current tab pad".into());
    }
    if !fs::metadata(&file).map_err(|e| format!("inspect --from-pad file: {e}"))?.is_file() {
        return Err("--from-pad source must be a regular file".into());
    }
    fs::read_to_string(file).map_err(|e| format!("read --from-pad as UTF-8: {e}"))
}

pub fn list_pad_files() -> Result<Vec<PathBuf>, String> {
    let root = current_pad()?;
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("list AgentPad: {e}"))? {
        let path = entry.map_err(|e| format!("list AgentPad entry: {e}"))?.path();
        let canonical = path.canonicalize().map_err(|e| format!("resolve AgentPad entry: {e}"))?;
        if !canonical.starts_with(root) { return Err("AgentPad entry escapes the current pad".into()); }
        let metadata = fs::metadata(&canonical).map_err(|e| format!("inspect AgentPad entry: {e}"))?;
        if metadata.is_dir() { collect(root, &canonical, files)?; }
        else if metadata.is_file() { files.push(canonical.strip_prefix(root).unwrap().to_path_buf()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_utf8_is_preserved_and_traversal_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("full-tab-id");
        fs::create_dir(&pad).unwrap();
        fs::create_dir(pad.join("nested")).unwrap();
        fs::write(pad.join("nested/report.md"), "quotes ` $ Привіт\n\n").unwrap();
        std::env::set_var("CODETERM_TAB_ID", "full-tab-id");
        std::env::set_var("CODETERM_PAD_DIR", &pad);
        assert_eq!(read_from_pad("nested/report.md").unwrap(), "quotes ` $ Привіт\n\n");
        assert!(read_from_pad("../other.md").is_err());
        assert!(read_from_pad("nested").is_err());
    }
}
