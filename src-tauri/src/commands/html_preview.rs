use base64::Engine as _;
use serde::Serialize;

const HTML_PREVIEW_FILE_LIMIT: u64 = 200_000;
const DOCUMENT_PREVIEW_FILE_LIMIT: u64 = 1_000_000;
const IMAGE_PREVIEW_FILE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct LocalPreviewFile {
    data: String,
    media_type: String,
}

fn canonical_preview_file(path: &str, label: &str) -> Result<std::path::PathBuf, String> {
    let requested = std::path::Path::new(path.trim());
    if !requested.is_absolute() {
        return Err(format!("{label} preview path must be absolute"));
    }
    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("Unable to resolve {label} preview file: {error}"))?;
    if !canonical
        .metadata()
        .map_err(|error| format!("Unable to inspect {label} preview file: {error}"))?
        .is_file()
    {
        return Err(format!("{label} preview source is not a file"));
    }
    Ok(canonical)
}

fn bounded_file(path: &std::path::Path, label: &str, limit: u64) -> Result<Vec<u8>, String> {
    let size = path
        .metadata()
        .map_err(|error| format!("Unable to inspect {label} preview file: {error}"))?
        .len();
    if size > limit {
        return Err(format!("{label} preview source exceeds {limit} bytes"));
    }
    std::fs::read(path).map_err(|error| format!("Unable to read {label} preview file: {error}"))
}

fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn read_html_preview_file_core(path: &str) -> Result<String, String> {
    let canonical = canonical_preview_file(path, "HTML")?;
    if !matches!(extension(&canonical).as_str(), "html" | "htm") {
        return Err("HTML preview source must use .html or .htm".into());
    }
    String::from_utf8(bounded_file(&canonical, "HTML", HTML_PREVIEW_FILE_LIMIT)?)
        .map_err(|error| format!("Unable to read HTML preview file as UTF-8: {error}"))
}

pub fn read_local_preview_file_core(path: &str, kind: &str) -> Result<LocalPreviewFile, String> {
    let canonical = canonical_preview_file(path, "Local")?;
    let ext = extension(&canonical);
    match kind {
        "page" => Ok(LocalPreviewFile {
            data: read_html_preview_file_core(path)?,
            media_type: "text/html".into(),
        }),
        "document" => {
            let media_type = match ext.as_str() {
                "md" | "markdown" => "text/markdown",
                "txt" | "log" => "text/plain",
                "json" => "application/json",
                "yaml" | "yml" => "application/yaml",
                "toml" => "application/toml",
                "csv" => "text/csv",
                _ => return Err("Document preview supports .md, .markdown, .txt, .log, .json, .yaml, .yml, .toml, or .csv".into()),
            };
            let bytes = bounded_file(&canonical, "Document", DOCUMENT_PREVIEW_FILE_LIMIT)?;
            Ok(LocalPreviewFile {
                data: String::from_utf8(bytes).map_err(|error| {
                    format!("Unable to read document preview as UTF-8: {error}")
                })?,
                media_type: media_type.into(),
            })
        }
        "image" => {
            let media_type = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "avif" => "image/avif",
                // Rendered only through <img>, where SVG scripts and external loads do not run.
                "svg" => "image/svg+xml",
                _ => return Err(
                    "Image preview supports .png, .jpg, .jpeg, .gif, .webp, .bmp, .avif, or .svg"
                        .into(),
                ),
            };
            let bytes = bounded_file(&canonical, "Image", IMAGE_PREVIEW_FILE_LIMIT)?;
            Ok(LocalPreviewFile {
                data: format!(
                    "data:{media_type};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
                media_type: media_type.into(),
            })
        }
        _ => Err("Local preview kind must be page, image, or document".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct LocalPreviewFiles {
    paths: Vec<String>,
    home: Option<String>,
}

fn check_local_preview_files_core(paths: Vec<String>) -> LocalPreviewFiles {
    LocalPreviewFiles {
        paths: paths
            .into_iter()
            .filter(|path| canonical_preview_file(path, "Local").is_ok())
            .collect(),
        home: dirs::home_dir().map(|path| path.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
pub async fn check_local_preview_files(paths: Vec<String>) -> Result<LocalPreviewFiles, String> {
    tokio::task::spawn_blocking(move || check_local_preview_files_core(paths))
        .await
        .map_err(|error| format!("Local preview inspection task failed: {error}"))
}

#[tauri::command]
pub async fn read_html_preview_file(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_html_preview_file_core(&path))
        .await
        .map_err(|error| format!("HTML preview read task failed: {error}"))?
}

#[tauri::command]
pub async fn read_local_preview_file(
    path: String,
    kind: String,
) -> Result<LocalPreviewFile, String> {
    tokio::task::spawn_blocking(move || read_local_preview_file_core(&path, &kind))
        .await
        .map_err(|error| format!("Local preview read task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_preview_check_excludes_missing_relative_and_directory_paths() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("notes.md");
        std::fs::write(&file, "# Notes").unwrap();
        let file = file.to_string_lossy().into_owned();
        let result = check_local_preview_files_core(vec![
            file.clone(),
            temp.path()
                .join("missing.md")
                .to_string_lossy()
                .into_owned(),
            temp.path().to_string_lossy().into_owned(),
            "relative.md".into(),
        ]);
        assert_eq!(result.paths, vec![file]);
    }

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

    #[test]
    fn reads_svg_images_as_svg_data_urls() {
        let temp = tempfile::tempdir().unwrap();
        let svg = temp.path().join("qr.svg");
        std::fs::write(&svg, "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();

        let image = read_local_preview_file_core(svg.to_str().unwrap(), "image").unwrap();
        assert_eq!(image.media_type, "image/svg+xml");
        assert!(image.data.starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn reads_bounded_documents_and_images_by_explicit_kind() {
        let temp = tempfile::tempdir().unwrap();
        let markdown = temp.path().join("notes.md");
        let image = temp.path().join("pixel.png");
        std::fs::write(&markdown, "# Notes").unwrap();
        std::fs::write(&image, [0x89, b'P', b'N', b'G']).unwrap();

        let document =
            read_local_preview_file_core(markdown.to_str().unwrap(), "document").unwrap();
        assert_eq!(document.data, "# Notes");
        assert_eq!(document.media_type, "text/markdown");

        let image = read_local_preview_file_core(image.to_str().unwrap(), "image").unwrap();
        assert_eq!(image.media_type, "image/png");
        assert!(image.data.starts_with("data:image/png;base64,"));
        assert!(read_local_preview_file_core(markdown.to_str().unwrap(), "image").is_err());
    }
}
