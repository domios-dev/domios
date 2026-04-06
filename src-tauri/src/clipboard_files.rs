//! Read file paths from the system clipboard.
//!
//! The web Clipboard API cannot expose file references (e.g. CF_HDROP on Windows,
//! NSFilenamesPboardType on macOS). This module provides native access so that
//! "copy file in Explorer → Ctrl+V in terminal" works like drag-and-drop.

/// Read a list of file paths currently on the clipboard, or an empty vec if none.
/// Cross-platform: Windows uses CF_HDROP, macOS/Linux return empty for now.
#[tauri::command]
pub async fn read_clipboard_files() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        read_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // TODO: macOS NSFilenamesPboardType via objc2-app-kit
        // TODO: Linux text/uri-list via xclip or gtk
        Ok(Vec::new())
    }
}

#[cfg(target_os = "windows")]
fn read_windows() -> Result<Vec<String>, String> {
    use clipboard_win::formats::FileList;
    use clipboard_win::{get_clipboard, Clipboard};

    // Try to open the clipboard (with internal retry loop).
    // If another process owns it briefly, this can fail — treat as "no files".
    let _owner = match Clipboard::new_attempts(10) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    drop(_owner); // get_clipboard below opens its own guard

    match get_clipboard(FileList) {
        Ok(files) => Ok(files),
        Err(_) => Ok(Vec::new()),
    }
}
