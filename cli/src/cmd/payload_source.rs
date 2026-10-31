use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub fn current_pad() -> Result<PathBuf, String> {
    let tab_id = crate::nonempty_env("CODETERM_TAB_ID")
        .ok_or("AgentPad requires CODETERM_TAB_ID; focused-tab fallback is not allowed")?;
    let (mut client, _) = crate::client::SocketClient::connect_local()?;
    resolve_current_pad(&tab_id, &mut client)
}

pub(crate) fn resolve_current_pad(
    tab_id: &str,
    client: &mut dyn crate::client::RpcClient,
) -> Result<PathBuf, String> {
    let resolved = client.call(
        "tab.agent_pad",
        Some(serde_json::json!({ "tab_id": tab_id })),
    )?;
    let path = PathBuf::from(
        resolved
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or("tab.agent_pad returned no path")?,
    );
    if !path.is_absolute() || path.file_name().and_then(|v| v.to_str()) != Some(tab_id) {
        return Err("resolved AgentPad does not belong to the current full tab id".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("resolve current AgentPad: {e}"))?;
    if !canonical.is_dir() {
        return Err("current AgentPad is not a directory".into());
    }
    Ok(canonical)
}

/// Display boundary only. Containment compares canonical paths component-wise,
/// so a stripped verbatim prefix must never reach `collect` or `current_pad`.
pub fn display_pad_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

pub fn read_from_pad(relative: &str) -> Result<String, String> {
    if relative == "-" {
        return read_file_or_stdin("-", "--from-pad");
    }
    read_from_pad_root(&current_pad()?, relative)
}

pub(crate) fn read_file_or_stdin(path: &str, option: &str) -> Result<String, String> {
    read_file_or_stdin_with(path, option, &mut std::io::stdin())
}

fn read_file_or_stdin_with(
    path: &str,
    option: &str,
    stdin: &mut impl Read,
) -> Result<String, String> {
    if path == "-" {
        let mut body = String::new();
        stdin
            .read_to_string(&mut body)
            .map_err(|e| format!("failed to read {option} from stdin: {e}"))?;
        Ok(body)
    } else {
        crate::local_admission::read_local_text(path).map_err(|e| format!("failed to read {option} {path}: {e}"))
    }
}

pub(crate) fn read_from_pad_root(root: &Path, relative: &str) -> Result<String, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        || (!relative.is_absolute()
            && relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_))))
    {
        return Err("--from-pad must be a non-empty file path without traversal".into());
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("resolve AgentPad root: {e}"))?;
    let file = canonical_root
        .join(relative)
        .canonicalize()
        .map_err(|e| format!("resolve --from-pad {}: {e}", relative.display()))?;
    if file == canonical_root || !file.starts_with(&canonical_root) {
        return Err("--from-pad path escapes the current tab pad".into());
    }
    if !fs::metadata(&file)
        .map_err(|e| format!("inspect --from-pad file: {e}"))?
        .is_file()
    {
        return Err("--from-pad source must be a regular file".into());
    }
    fs::read_to_string(file).map_err(|e| format!("read --from-pad as UTF-8: {e}"))
}

/// Pre-flight for an argv report body, run before the first socket call so a
/// refused report costs one error instead of a round-trip and a mutation.
pub fn refuse_inline_report_body(body: Option<&str>) {
    crate::cmd::quoted_handoff::refuse_inline(
        crate::cmd::quoted_handoff::INLINE_SOURCE,
        body.unwrap_or_default(),
        crate::cmd::quoted_handoff::PAD_REMEDY,
    );
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
        let path = entry
            .map_err(|e| format!("list AgentPad entry: {e}"))?
            .path();
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("resolve AgentPad entry: {e}"))?;
        if !canonical.starts_with(root) {
            return Err("AgentPad entry escapes the current pad".into());
        }
        let metadata =
            fs::metadata(&canonical).map_err(|e| format!("inspect AgentPad entry: {e}"))?;
        if metadata.is_dir() {
            collect(root, &canonical, files)?;
        } else if metadata.is_file() {
            let relative = canonical.strip_prefix(root).unwrap();
            if relative.to_str().is_none() {
                return Err("AgentPad file name is not UTF-8".into());
            }
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
