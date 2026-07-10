//! Native OS ownership for the external crash supervisor.

pub fn is_enabled() -> bool {
    platform::is_enabled()
}

pub fn enable() -> Result<(), String> {
    mark_supervisor_enabled()?;
    platform::enable()
}

pub fn disable() -> Result<(), String> {
    mark_supervisor_disabled()?;
    platform::disable()
}

fn supervisor_disabled_path() -> std::path::PathBuf {
    crate::paths::data_dir()
        .join("state")
        .join("recovery")
        .join("supervisor.disabled")
}

fn mark_supervisor_enabled() -> Result<(), String> {
    match std::fs::remove_file(supervisor_disabled_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn mark_supervisor_disabled() -> Result<(), String> {
    let path = supervisor_disabled_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, b"disabled\n").map_err(|error| error.to_string())
}

fn cli_path() -> Result<std::path::PathBuf, String> {
    let name = if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    };
    let home = dirs::home_dir().ok_or_else(|| "home dir not found".to_string())?;
    for candidate in [
        home.join(".codeterm").join("bin").join(name),
        home.join(".codeterm-dev").join("bin").join(name),
    ] {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("codeterm CLI is not installed yet; restart CodeTerm and try again".into())
}

#[cfg(target_os = "windows")]
mod platform {
    const TASK_NAME: &str = "CodeTerm-Autostart";

    pub fn is_enabled() -> bool {
        run(&["/Query", "/TN", TASK_NAME, "/FO", "CSV", "/NH"])
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn enable() -> Result<(), String> {
        let app = std::env::current_exe().map_err(|error| error.to_string())?;
        let supervisor = super::cli_path()?;
        let command = format!(
            "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"Start-Process -WindowStyle Hidden -FilePath '{}' -ArgumentList @('system','supervise','--binary','{}')\"",
            supervisor.to_string_lossy().replace('\'', "''"),
            app.to_string_lossy().replace('\'', "''")
        );
        let _ = run(&["/Delete", "/TN", TASK_NAME, "/F"]);
        checked(&[
            "/Create", "/TN", TASK_NAME, "/TR", &command, "/SC", "ONLOGON", "/F", "/RL", "LIMITED",
        ])?;
        checked(&["/Run", "/TN", TASK_NAME])
    }

    pub fn disable() -> Result<(), String> {
        let _ = run(&["/End", "/TN", TASK_NAME]);
        let _ = run(&["/Delete", "/TN", TASK_NAME, "/F"]);
        Ok(())
    }

    fn checked(args: &[&str]) -> Result<(), String> {
        let output = run(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    fn run(args: &[&str]) -> Result<std::process::Output, String> {
        let mut command = std::process::Command::new("schtasks");
        command.args(args);
        crate::process_util::no_window_std(&mut command);
        command.output().map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    const LABEL: &str = "com.codeterm.app";

    pub fn is_enabled() -> bool {
        plist_path().exists()
    }

    pub fn enable() -> Result<(), String> {
        let app = xml_escape(
            &std::env::current_exe()
                .map_err(|error| error.to_string())?
                .to_string_lossy(),
        );
        let supervisor = xml_escape(&super::cli_path()?.to_string_lossy());
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{LABEL}</string><key>ProgramArguments</key><array><string>{supervisor}</string><string>system</string><string>supervise</string><string>--binary</string><string>{app}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><false/><key>ProcessType</key><string>Interactive</string></dict></plist>\n"
        );
        let path = plist_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, body).map_err(|error| error.to_string())?;
        let _ = launchctl(&["unload", &path.to_string_lossy()]);
        checked_launchctl(&["load", &path.to_string_lossy()])
    }

    pub fn disable() -> Result<(), String> {
        let path = plist_path();
        let _ = launchctl(&["unload", &path.to_string_lossy()]);
        if path.exists() {
            std::fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn plist_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/LaunchAgents/com.codeterm.app.plist")
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn checked_launchctl(args: &[&str]) -> Result<(), String> {
        let output = launchctl(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    fn launchctl(args: &[&str]) -> Result<std::process::Output, String> {
        std::process::Command::new("launchctl")
            .args(args)
            .output()
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    const UNIT: &str = "codeterm.service";

    pub fn is_enabled() -> bool {
        unit_path().exists()
    }

    pub fn enable() -> Result<(), String> {
        let app = quote(
            &std::env::current_exe()
                .map_err(|error| error.to_string())?
                .to_string_lossy(),
        );
        let supervisor = quote(&super::cli_path()?.to_string_lossy());
        let body = format!(
            "[Unit]\nDescription=CodeTerm crash supervisor\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={supervisor} system supervise --binary {app}\nRestart=no\n\n[Install]\nWantedBy=default.target\n"
        );
        let path = unit_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, body).map_err(|error| error.to_string())?;
        checked_systemctl(&["daemon-reload"])?;
        checked_systemctl(&["enable", UNIT])?;
        checked_systemctl(&["start", UNIT])
    }

    pub fn disable() -> Result<(), String> {
        let _ = systemctl(&["stop", UNIT]);
        let _ = systemctl(&["disable", UNIT]);
        let path = unit_path();
        if path.exists() {
            std::fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        let _ = systemctl(&["daemon-reload"]);
        Ok(())
    }

    fn unit_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config/systemd/user")
            .join(UNIT)
    }

    fn quote(value: &str) -> String {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }

    fn checked_systemctl(args: &[&str]) -> Result<(), String> {
        let output = systemctl(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    fn systemctl(args: &[&str]) -> Result<std::process::Output, String> {
        std::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .map_err(|error| error.to_string())
    }
}
