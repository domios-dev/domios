use codeterm_service_core::attachments::CHAT_UPLOAD_CACHE_BYTES;

pub fn save_clipboard_image_core(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let path = codeterm_service_core::attachments::save_with_uuid(
        "images",
        &ext,
        &bytes,
        codeterm_service_core::attachments::EvictionPolicy::MaxCount(20),
    )?;
    Ok(path.to_string_lossy().to_string())
}

pub fn save_clipboard_file_core(bytes: Vec<u8>, filename: String) -> Result<String, String> {
    let path = codeterm_service_core::attachments::save_with_prefixed_name(
        "files",
        &filename,
        &bytes,
        // A single chat upload may reach 150 MB, so the shared 100 MB cap would evict it at once.
        codeterm_service_core::attachments::EvictionPolicy::MaxBytes(CHAT_UPLOAD_CACHE_BYTES),
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn materialize_dropped_files(paths: Vec<String>) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                codeterm_service_core::attachments::materialize_dropped_path(std::path::Path::new(
                    &path,
                ))
                .map(|p| p.to_string_lossy().to_string())
            })
            .collect()
    })
    .await
    .map_err(|e| format!("materialize dropped files task failed: {}", e))?
}

#[tauri::command]
pub async fn copy_image_to_clipboard(path: String) -> Result<(), String> {
    // off the async runtime so it can't stall IPC (mirrors materialize_dropped_files).
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let escaped_path = path.replace("'", "''");
            let ps_script = format!(
                r#"
            Add-Type -AssemblyName System.Windows.Forms
            Add-Type -AssemblyName System.Drawing
            $img = [System.Drawing.Image]::FromFile('{escaped_path}')
            $data = New-Object System.Windows.Forms.DataObject
            $data.SetImage($img)
            $ms = New-Object System.IO.MemoryStream
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $ms.Position = 0
            $data.SetData('PNG', $false, $ms)
            [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)
            $img.Dispose()
            "#
            );
            log::debug!("[clipboard] copying image to clipboard: {}", path);
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-STA", "-Command", &ps_script]);
            codeterm_service_core::process_util::no_window_std(&mut cmd);
            let output = cmd
                .output()
                .map_err(|e| format!("PowerShell launch failed: {}", e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::error!("[clipboard] PowerShell failed: {}", stderr);
                return Err(format!("Clipboard copy failed: {}", stderr));
            }
            log::debug!("[clipboard] image copied to clipboard successfully");
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
                path.replace("\"", "\\\"")
            );
            let output = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                let script2 = format!(
                    "set the clipboard to (read (POSIX file \"{}\") as TIFF picture)",
                    path.replace("\"", "\\\"")
                );
                let output2 = std::process::Command::new("osascript")
                    .args(["-e", &script2])
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output2.status.success() {
                    let stderr = String::from_utf8_lossy(&output2.stderr);
                    return Err(format!("Clipboard copy failed: {}", stderr));
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            let output = std::process::Command::new("xclip")
                .args([
                    "-selection",
                    "clipboard",
                    "-target",
                    "image/png",
                    "-i",
                    &path,
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err("xclip failed".to_string());
            }
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("copy_image_to_clipboard task panicked: {e}"))?
}
