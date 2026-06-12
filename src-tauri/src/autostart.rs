//! OS-level autostart management.
//!
//! - Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! - macOS: ~/Library/LaunchAgents/com.codeterm.app.plist
//! - Linux: ~/.config/autostart/codeterm.desktop

pub fn is_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_enabled()
    }
    #[cfg(target_os = "macos")]
    {
        macos::is_enabled()
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_enabled()
    }
}

pub fn enable() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows::enable()
    }
    #[cfg(target_os = "macos")]
    {
        macos::enable()
    }
    #[cfg(target_os = "linux")]
    {
        linux::enable()
    }
}

pub fn disable() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows::disable()
    }
    #[cfg(target_os = "macos")]
    {
        macos::disable()
    }
    #[cfg(target_os = "linux")]
    {
        linux::disable()
    }
}

#[cfg(target_os = "windows")]
mod windows {
    const REG_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "CodeTerm";

    pub fn is_enabled() -> bool {
        use std::process::Command;
        let mut cmd = Command::new("reg");
        cmd.args(["query", &format!("HKCU\\{}", REG_KEY), "/v", VALUE_NAME]);
        crate::process_util::no_window_std(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    }

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        use std::process::Command;
        let mut cmd = Command::new("reg");
        cmd.args([
            "add",
            &format!("HKCU\\{}", REG_KEY),
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &format!("\"{}\" --minimized", exe.to_string_lossy()),
            "/f",
        ]);
        crate::process_util::no_window_std(&mut cmd);
        let output = cmd.output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    pub fn disable() -> Result<(), String> {
        use std::process::Command;
        let mut cmd = Command::new("reg");
        cmd.args([
            "delete",
            &format!("HKCU\\{}", REG_KEY),
            "/v",
            VALUE_NAME,
            "/f",
        ]);
        crate::process_util::no_window_std(&mut cmd);
        let _ = cmd.output();
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;
    fn plist_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/LaunchAgents/com.codeterm.app.plist")
    }

    pub fn is_enabled() -> bool {
        plist_path().exists()
    }

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.codeterm.app</string>
    <key>ProgramArguments</key><array><string>{}</string><string>--minimized</string></array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><false/>
</dict>
</plist>"#,
            exe.to_string_lossy()
        );
        let path = plist_path();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, plist).map_err(|e| e.to_string())
    }

    pub fn disable() -> Result<(), String> {
        let path = plist_path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    fn desktop_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config/autostart/codeterm.desktop")
    }

    pub fn is_enabled() -> bool {
        desktop_path().exists()
    }

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let content = format!("[Desktop Entry]\nType=Application\nName=CodeTerm\nExec={} --minimized\nHidden=false\nX-GNOME-Autostart-enabled=true\n", exe.to_string_lossy());
        let path = desktop_path();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }

    pub fn disable() -> Result<(), String> {
        let path = desktop_path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
