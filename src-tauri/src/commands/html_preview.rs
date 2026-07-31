const HTML_PREVIEW_FILE_LIMIT: u64 = 200_000;

pub fn read_html_preview_file_core(path: &str) -> Result<String, String> {
    let requested = std::path::Path::new(path.trim());
    if !requested.is_absolute() {
        return Err("HTML preview path must be absolute".into());
    }
    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("Unable to resolve HTML preview file: {error}"))?;
    let extension = canonical
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("html") && !extension.eq_ignore_ascii_case("htm") {
        return Err("HTML preview source must use .html or .htm".into());
    }
    let metadata = canonical
        .metadata()
        .map_err(|error| format!("Unable to inspect HTML preview file: {error}"))?;
    if !metadata.is_file() {
        return Err("HTML preview source is not a file".into());
    }
    if metadata.len() > HTML_PREVIEW_FILE_LIMIT {
        return Err(format!(
            "HTML preview source exceeds {HTML_PREVIEW_FILE_LIMIT} bytes"
        ));
    }
    std::fs::read_to_string(&canonical)
        .map_err(|error| format!("Unable to read HTML preview file as UTF-8: {error}"))
}

#[tauri::command]
pub async fn read_html_preview_file(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_html_preview_file_core(&path))
        .await
        .map_err(|error| format!("HTML preview read task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_bounded_absolute_html_files() {
        let temp = tempfile::tempdir().unwrap();
        let html = temp.path().join("view.html");
        std::fs::write(&html, "<main>safe</main>").unwrap();

        assert_eq!(
            read_html_preview_file_core(html.to_str().unwrap()).unwrap(),
            "<main>safe</main>"
        );
        assert!(read_html_preview_file_core("relative.html")
            .unwrap_err()
            .contains("absolute"));
    }

    #[test]
    fn rejects_non_html_and_oversized_files() {
        let temp = tempfile::tempdir().unwrap();
        let text = temp.path().join("view.txt");
        std::fs::write(&text, "not html").unwrap();
        assert!(read_html_preview_file_core(text.to_str().unwrap())
            .unwrap_err()
            .contains(".html"));

        let oversized = temp.path().join("large.html");
        std::fs::write(&oversized, vec![b'x'; HTML_PREVIEW_FILE_LIMIT as usize + 1]).unwrap();
        assert!(read_html_preview_file_core(oversized.to_str().unwrap())
            .unwrap_err()
            .contains("exceeds"));
    }
}
