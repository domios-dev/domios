use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn current_pad() -> Result<PathBuf, String> {
    let tab_id = std::env::var("CODETERM_TAB_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or("AgentPad requires CODETERM_TAB_ID; focused-tab fallback is not allowed")?;
    let socket = crate::client::discover_socket()?;
    let mut client = crate::client::SocketClient::connect(&socket)?;
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
    if !path.is_absolute() || path.file_name().and_then(|v| v.to_str()) != Some(tab_id.as_str()) {
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
    read_from_pad_root(&current_pad()?, relative)
}

pub(crate) fn read_from_pad_root(root: &Path, relative: &str) -> Result<String, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("--from-pad must be a non-empty relative file path without traversal".into());
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

/// No prescribed template inlines a report body any more; past this an argv body
/// is an authoring mistake, not a deliberate short call.
pub const INLINE_BODY_REFUSAL_LIMIT: usize = 4000;

/// Cheap, local pre-flight for an argv report body. Returns the refusal text so
/// an oversized paste costs one error instead of a failed mutation.
pub fn oversize_inline_refusal(body: Option<&str>) -> Option<String> {
    let count = body?.chars().count();
    if count <= INLINE_BODY_REFUSAL_LIMIT {
        return None;
    }
    Some(format!(
        "Error: this report body is {count} characters and went inline through the shell, which \
         silently mangles backticks, $, quotes, and newlines. Refused before the report was sent.\n\
         Required: stage it in the pad (`codeterm pad path`) and pass --from-pad."
    ))
}

/// Must run before the first socket call so the refusal never pays for a
/// round-trip, a task lookup, or the report mutation.
pub fn refuse_oversize_inline(body: Option<&str>) {
    if let Some(refusal) = oversize_inline_refusal(body) {
        eprintln!("{refusal}");
        std::process::exit(1);
    }
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
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeDestination {
        payload: Option<String>,
        status: &'static str,
    }

    impl FakeDestination {
        fn accept(&mut self, payload: String, status: &'static str) {
            self.payload = Some(payload);
            self.status = status;
        }
    }

    #[test]
    fn nested_utf8_is_preserved_and_traversal_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("full-tab-id");
        fs::create_dir(&pad).unwrap();
        fs::create_dir(pad.join("nested")).unwrap();
        fs::write(pad.join("nested/report.md"), "quotes ` $ Привіт\n\n").unwrap();
        assert_eq!(
            read_from_pad_root(&pad, "nested/report.md").unwrap(),
            "quotes ` $ Привіт\n\n"
        );
        assert!(read_from_pad_root(&pad, "../other.md").is_err());
        assert!(read_from_pad_root(&pad, "nested").is_err());
        assert_eq!(
            read_from_pad_root(&pad, "nested/report.md").unwrap(),
            "quotes ` $ Привіт\n\n"
        );
    }

    #[test]
    fn oversize_inline_report_body_is_refused_with_the_staging_remedy() {
        let body = "x".repeat(INLINE_BODY_REFUSAL_LIMIT + 1);
        let refusal =
            oversize_inline_refusal(Some(&body)).expect("an oversized argv body is refused");
        assert!(refusal.contains("--from-pad"));
        assert!(refusal.contains("codeterm pad path"));
        assert!(refusal.contains(&(INLINE_BODY_REFUSAL_LIMIT + 1).to_string()));
    }

    /// Negative control: the shapes real callers inline today stay accepted.
    #[test]
    fn prescribed_and_scalar_inline_report_bodies_are_not_refused() {
        let worker_template = "STATUS: in_review\nCHANGES: cli/src/cmd/payload_source.rs\n\
             TESTS RUN: deferred to Orchestrator canonical QA\nGATES REQUESTED: none\n\
             FOLLOWUP: Orchestrator reviews and closes the task";
        assert!(oversize_inline_refusal(Some(worker_template)).is_none());
        assert!(oversize_inline_refusal(Some("Lifecycle milestone appended")).is_none());
        assert!(oversize_inline_refusal(Some(&"y".repeat(INLINE_BODY_REFUSAL_LIMIT))).is_none());
        assert!(oversize_inline_refusal(None).is_none());
    }

    #[test]
    fn oversize_refusal_counts_characters_not_bytes() {
        // Two bytes per char, so a byte-counting guard would refuse at the limit.
        let body = "и".repeat(INLINE_BODY_REFUSAL_LIMIT);
        assert_eq!(body.chars().count(), INLINE_BODY_REFUSAL_LIMIT);
        assert!(body.len() > INLINE_BODY_REFUSAL_LIMIT);
        assert!(oversize_inline_refusal(Some(&body)).is_none());
    }

    #[test]
    fn current_pad_never_falls_back_without_current_tab_identity() {
        let old = std::env::var_os("CODETERM_TAB_ID");
        std::env::remove_var("CODETERM_TAB_ID");
        assert!(current_pad()
            .unwrap_err()
            .contains("focused-tab fallback is not allowed"));
        if let Some(value) = old {
            std::env::set_var("CODETERM_TAB_ID", value);
        }
    }

    #[test]
    fn pad_enumeration_keeps_the_canonical_root_display_normalization_would_break() {
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("containment-full-tab-id");
        fs::create_dir(&pad).unwrap();
        fs::create_dir(pad.join("nested")).unwrap();
        fs::write(pad.join("nested/report.md"), "body").unwrap();
        let canonical = pad.canonicalize().unwrap();

        let mut files = Vec::new();
        collect(&canonical, &canonical, &mut files).unwrap();
        assert_eq!(files, vec![PathBuf::from("nested").join("report.md")]);

        let displayed = PathBuf::from(display_pad_path(&canonical));
        let mut from_displayed_root = Vec::new();
        let result = collect(&displayed, &canonical, &mut from_displayed_root);
        if cfg!(windows) {
            assert_eq!(
                result.unwrap_err(),
                "AgentPad entry escapes the current pad",
                "the display form is not a containment root on Windows"
            );
        } else {
            assert_eq!(displayed, canonical);
            result.unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_before_read() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("full-tab-symlink");
        fs::create_dir(&pad).unwrap();
        let outside = temp.path().join("outside.md");
        fs::write(&outside, "outside").unwrap();
        symlink(outside, pad.join("escape.md")).unwrap();
        assert!(read_from_pad_root(&pad, "escape.md").is_err());
    }

    #[test]
    fn send_from_pad_resolves_before_transport() {
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("send-owner-full-id");
        fs::create_dir(&pad).unwrap();
        let expected = "reusable `payload` $value Привіт\nsecond line\n";
        fs::write(pad.join("message.md"), expected).unwrap();
        let first = read_from_pad_root(&pad, "message.md").unwrap();
        let second = read_from_pad_root(&pad, "message.md").unwrap();
        let mut transport = FakeDestination::default();
        transport.accept(first, "accepted");
        assert_eq!(transport.payload.as_deref(), Some(expected));
        assert_eq!(second, expected);
        assert!(!transport
            .payload
            .as_ref()
            .unwrap()
            .contains(pad.to_string_lossy().as_ref()));
    }

    #[test]
    fn agent_pad_report_reaches_parent_and_survives_worker_close() {
        let temp = tempfile::tempdir().unwrap();
        let pad = temp.path().join("worker-full-id");
        fs::create_dir(&pad).unwrap();
        let expected = "STATUS: in_review\n\n- quotes: '\"'\n- code: `x`\n- dollar: $HOME\n- Unicode: Привіт\n";
        fs::write(pad.join("report.md"), expected).unwrap();
        let resolved = read_from_pad_root(&pad, "report.md").unwrap();
        let mut parent = FakeDestination::default();
        parent.accept(resolved, "in_review");
        fs::remove_dir_all(&pad).unwrap();
        assert_eq!(parent.payload.as_deref(), Some(expected));
        assert_eq!(parent.status, "in_review");
        assert!(!pad.exists());
    }
}
