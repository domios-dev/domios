//! Native clipboard file reference reading.
//!
//! Web Clipboard API exposes only text and image data. File references
//! (e.g. "Copy file in Explorer/Finder") use OS-specific formats:
//! - Windows: CF_HDROP
//! - macOS: NSPasteboardTypeFileURL
//! - Linux: text/uri-list (TODO)

#[tauri::command]
pub async fn read_clipboard_files(
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    // app is only consumed by the macOS NSPasteboard main-thread hop;
    // suppress the unused-warning on other platforms instead of cfg-gating
    // the parameter itself — Tauri's generate_handler! expands to a fixed
    // arity and rejects a parameter that disappears under #[cfg].
    let _ = &app;

    #[cfg(target_os = "windows")]
    let paths = read_windows();
    #[cfg(target_os = "macos")]
    let paths = read_macos_on_main(&app)?;
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let paths: Vec<String> = Vec::new();

    Ok(paths.into_iter().map(sanitize_path).collect())
}

/// Hop to main thread before touching NSPasteboard — AppKit pasteboard APIs
/// are not thread-safe and racing against WebKit's paste path corrupts the
/// type cache (`__NSArrayM insertObject:atIndex:` → SIGABRT). See issue #34.
#[cfg(target_os = "macos")]
fn read_macos_on_main(app: &tauri::AppHandle) -> Result<Vec<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(read_macos());
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

/// Strip control characters — prevents newline injection from triggering auto-submit.
fn sanitize_path(p: String) -> String {
    p.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(target_os = "windows")]
fn read_windows() -> Vec<String> {
    use clipboard_win::formats::FileList;
    use clipboard_win::get_clipboard;

    match get_clipboard::<Vec<String>, _>(FileList) {
        Ok(files) => {
            log::info!("[clipboard] files (win) → {} items", files.len());
            files
        }
        Err(e) => {
            log::info!("[clipboard] files (win) err: {:?}", e);
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
fn read_macos() -> Vec<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};

    let pb = NSPasteboard::generalPasteboard();
    let items = match pb.pasteboardItems() {
        Some(i) => i,
        None => return Vec::new(),
    };

    let mut paths = Vec::new();
    for item in items.iter() {
        let url_str = unsafe { item.stringForType(&NSPasteboardTypeFileURL) };
        let Some(url_str) = url_str else { continue };
        if let Some(path) = file_url_to_path(&url_str.to_string()) {
            paths.push(path);
        }
    }
    log::info!("[clipboard] files (mac) → {} items", paths.len());
    paths
}

#[cfg(target_os = "macos")]
fn file_url_to_path(url: &str) -> Option<String> {
    let trimmed = url.strip_prefix("file://")?;
    Some(percent_decode(trimmed))
}

#[cfg(target_os = "macos")]
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::{file_url_to_path, percent_decode};

    #[cfg(target_os = "macos")]
    #[test]
    fn file_url_strips_scheme() {
        assert_eq!(
            file_url_to_path("file:///Users/me/photo.png"),
            Some("/Users/me/photo.png".into())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_url_decodes_percent() {
        assert_eq!(
            file_url_to_path("file:///Users/me/my%20photo.png"),
            Some("/Users/me/my photo.png".into())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn percent_decode_passthrough() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%20"), " ");
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("unfinished%2"), "unfinished%2");
    }
}
