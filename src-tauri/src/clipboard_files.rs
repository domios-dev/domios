//! Native clipboard reading via the OS pasteboard.
//!
//! Bypasses the WebView's `navigator.clipboard` API entirely, which on macOS
//! WebKit is gated to "must be called inside a paste-event handler" — every
//! keydown / right-click / programmatic path we route from `routeClipboardToTerm`
//! is OUTSIDE that gate, so readText() silently returns nothing (and in
//! macOS Sequoia briefly shows a permission toast). Reading the system
//! pasteboard directly from Rust avoids the gate and matches what
//! `pbpaste` / any native app does.
//!
//! Text:  Windows CF_UNICODETEXT / macOS NSPasteboardTypeString
//! Files: Windows CF_HDROP / macOS NSPasteboardTypeFileURL
//! Linux: text/uri-list (TODO)

#[tauri::command]
pub async fn read_clipboard_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    // app is only consumed by the macOS NSPasteboard main-thread hop; suppress the unused-warning on other platforms instead of cfg-gating the
    // parameter itself — Tauri's generate_handler! expands to a fixed arity and rejects a parameter that disappears under #[cfg].
    let _ = &app;

    #[cfg(target_os = "windows")]
    let paths = read_windows();
    #[cfg(target_os = "macos")]
    let paths = read_macos_on_main(&app)?;
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let paths: Vec<String> = Vec::new();

    Ok(paths.into_iter().map(sanitize_path).collect())
}

/// Read plain text from the OS pasteboard. Returns `None` when there is no
/// text on the pasteboard (caller falls back to the web clipboard / file
/// paths). Used as the primary text source on macOS where WebKit blocks
/// `navigator.clipboard.readText()` outside paste-event handlers.
#[tauri::command]
pub async fn read_clipboard_text(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let _ = &app;

    #[cfg(target_os = "windows")]
    let text = read_windows_text();
    #[cfg(target_os = "macos")]
    let text = read_macos_text_on_main(&app)?;
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let text: Option<String> = None;

    Ok(text)
}

#[cfg(target_os = "macos")]
fn read_macos_text_on_main(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(read_macos_text());
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn read_macos_text() -> Option<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    let pb = NSPasteboard::generalPasteboard();
    let s = unsafe { pb.stringForType(&NSPasteboardTypeString) }?;
    let out = s.to_string();
    log::info!("[clipboard] text (mac) → {} chars", out.chars().count());
    Some(out)
}

#[cfg(target_os = "windows")]
fn read_windows_text() -> Option<String> {
    use clipboard_win::formats::Unicode;
    use clipboard_win::get_clipboard;
    match get_clipboard::<String, _>(Unicode) {
        Ok(s) if !s.is_empty() => {
            log::info!("[clipboard] text (win) → {} chars", s.chars().count());
            Some(s)
        }
        Ok(_) => None,
        Err(e) => {
            log::info!("[clipboard] text (win) err: {:?}", e);
            None
        }
    }
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
            log::info!(
                "[clipboard] files (win) → {} items: {:?}",
                files.len(),
                files
            );
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
        let raw = url_str.to_string();
        if let Some(path) = file_url_to_path(&raw) {
            paths.push(path);
        } else {
            log::warn!("[clipboard] dropped unparseable file url: {:?}", raw);
        }
    }
    log::info!(
        "[clipboard] files (mac) → {} items: {:?}",
        paths.len(),
        paths
    );
    paths
}

#[cfg(target_os = "macos")]
fn file_url_to_path(url: &str) -> Option<String> {
    use objc2_foundation::{NSString, NSURL};
    let ns = NSString::from_str(url);
    let nsurl = unsafe { NSURL::URLWithString(&ns) }?;
    let resolved = unsafe { nsurl.filePathURL() }.unwrap_or(nsurl);
    let path = unsafe { resolved.path() }?;
    Some(path.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::file_url_to_path;

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
    fn file_url_handles_cyrillic() {
        let url = "file:///Users/me/%D0%A1%D0%BD%D0%B8%D0%BC%D0%BE%D0%BA.png";
        assert_eq!(file_url_to_path(url), Some("/Users/me/Снимок.png".into()));
    }
}
