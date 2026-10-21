#[tauri::command]
pub fn get_windows_build_number() -> u32 {
    // Build number is constant for the OS lifetime — cache to avoid spawning cmd.exe on every TerminalTab mount (~1s on Win11 with Defender).
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            let mut cmd = Command::new("cmd");
            cmd.args(["/c", "ver"]);
            codeterm_util::process_util::no_window_std(&mut cmd);
            if let Ok(output) = cmd.output() {
                let ver = String::from_utf8_lossy(&output.stdout);
                if let Some(start) = ver.rfind('.') {
                    let before_last = &ver[..start];
                    if let Some(build_start) = before_last.rfind('.') {
                        if let Ok(n) = before_last[build_start + 1..].trim().parse::<u32>() {
                            return n;
                        }
                    }
                }
            }
            19041
        }
        #[cfg(not(target_os = "windows"))]
        {
            0
        }
    })
}
