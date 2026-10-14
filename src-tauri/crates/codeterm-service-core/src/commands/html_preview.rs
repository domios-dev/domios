use base64::Engine as _;
use serde::Serialize;

mod file;
pub(crate) use file::{checked_path, PreviewSource};

const HTML_PREVIEW_FILE_LIMIT: u64 = 200_000;
const DOCUMENT_PREVIEW_FILE_LIMIT: u64 = 1_000_000;
const IMAGE_PREVIEW_FILE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct LocalPreviewFile {
    pub data: String,
    pub media_type: String,
}

fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn media(path: &std::path::Path, kind: &str) -> Result<(&'static str, u64), String> {
    let ext = extension(path);
    let result = match (kind, ext.as_str()) {
        ("page", "html" | "htm") => ("text/html", HTML_PREVIEW_FILE_LIMIT),
        ("document", "md" | "markdown") => ("text/markdown", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("document", "txt" | "log") => ("text/plain", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("document", "json") => ("application/json", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("document", "yaml" | "yml") => ("application/yaml", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("document", "toml") => ("application/toml", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("document", "csv") => ("text/csv", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("image", "png") => ("image/png", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "jpg" | "jpeg") => ("image/jpeg", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "gif") => ("image/gif", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "webp") => ("image/webp", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "bmp") => ("image/bmp", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "avif") => ("image/avif", IMAGE_PREVIEW_FILE_LIMIT),
        ("image", "svg") => ("image/svg+xml", IMAGE_PREVIEW_FILE_LIMIT),
        ("page", _) => return Err("HTML preview source must use .html or .htm".into()),
        ("document", _) => ("text/plain", DOCUMENT_PREVIEW_FILE_LIMIT),
        ("image", _) => return Err("Unsupported local preview file extension".into()),
        _ => return Err("Local preview kind must be page, image, or document".into()),
    };
    Ok(result)
}

fn preview_text(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.contains(&0) {
        return Err("Binary file: text preview is unavailable".into());
    }
    String::from_utf8(bytes)
        .map_err(|_| "Binary or non-UTF-8 file: text preview is unavailable".to_string())
}

pub(crate) fn read_local_preview_file_guarded(
    path: &str,
    kind: &str,
    guard: impl Fn(&std::path::Path, Option<&std::path::Path>) -> Result<(), String>,
) -> Result<LocalPreviewFile, String> {
    let mut source = PreviewSource::open(path, &guard)?;
    let (media_type, limit) = media(&source.path, kind)?;
    let bytes = source.read(limit)?;
    let data = if kind == "image" {
        format!(
            "data:{media_type};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    } else {
        preview_text(bytes)?
    };
    Ok(LocalPreviewFile {
        data,
        media_type: media_type.into(),
    })
}

#[derive(Debug, Serialize)]
pub struct LocalPreviewFiles {
    pub paths: Vec<String>,
    pub home: Option<String>,
}

pub(crate) fn check_local_preview_files_guarded(
    paths: Vec<String>,
    guard: impl Fn(&std::path::Path, Option<&std::path::Path>) -> Result<(), String>,
) -> LocalPreviewFiles {
    let paths = paths
        .into_iter()
        .filter(|path| {
            let Ok(mut source) = PreviewSource::open(path, &guard) else {
                log::debug!("[local-preview] inspection refused");
                return false;
            };
            if media(&source.path, "image").is_ok_and(|(_, limit)| source.size <= limit) {
                return true;
            }
            source
                .read(DOCUMENT_PREVIEW_FILE_LIMIT)
                .and_then(preview_text)
                .is_ok()
        })
        .collect();
    LocalPreviewFiles {
        paths,
        home: dirs::home_dir().map(|path| path.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn preview_test_root(path: &std::path::Path) -> std::path::PathBuf {
    let canonical = path.canonicalize().unwrap();
    let text = canonical.to_string_lossy();
    std::path::PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}
