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
        let username = std::env::var("USERNAME").map_err(|error| error.to_string())?;
        let user = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!("{domain}\\{username}"),
            _ => username,
        };
        let xml = task_xml(&user, &supervisor.to_string_lossy(), &app.to_string_lossy());
        let path = std::env::temp_dir().join(format!("{TASK_NAME}-{}.xml", std::process::id()));
        std::fs::write(&path, utf16le_with_bom(&xml)).map_err(|error| error.to_string())?;
        let _ = run(&["/Delete", "/TN", TASK_NAME, "/F"]);
        let created = checked(&[
            "/Create",
            "/TN",
            TASK_NAME,
            "/XML",
            &path.to_string_lossy(),
            "/F",
        ]);
        let _ = std::fs::remove_file(&path);
        created?;
        checked(&["/Run", "/TN", TASK_NAME])
    }

    // schtasks /TR rejects commands over 261 characters; task XML has no such limit.
    fn task_xml(user: &str, supervisor: &str, app: &str) -> String {
        let arguments = format!(
            "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"Start-Process -WindowStyle Hidden -FilePath '{}' -ArgumentList @('system','supervise','--binary','{}')\"",
            supervisor.replace('\'', "''"),
            app.replace('\'', "''")
        );
        let user = xml_escape(user);
        let arguments = xml_escape(&arguments);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>\n<Principals><Principal id=\"Author\"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Enabled>true</Enabled></Settings>\n<Actions Context=\"Author\"><Exec><Command>powershell.exe</Command><Arguments>{arguments}</Arguments></Exec></Actions>\n</Task>\n"
        )
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn utf16le_with_bom(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn long_install_paths_stay_in_task_xml_arguments() {
            let long_dir = format!("C:\\Users\\{}\\AppData\\Local", "a".repeat(200));
            let supervisor = format!("{long_dir}\\bin\\codeterm & co.exe");
            let app = format!("{long_dir}\\codeterm\\codeterm.exe");
            let xml = task_xml("DOMAIN\\user", &supervisor, &app);

            let start = xml.find("<Arguments>").unwrap() + "<Arguments>".len();
            let end = xml.find("</Arguments>").unwrap();
            let arguments = xml[start..end]
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&");
            assert!(arguments.len() > 261);
            assert!(arguments.contains(&supervisor));
            assert!(arguments.contains(&app));
            assert!(xml.contains("<UserId>DOMAIN\\user</UserId>"));
        }

        #[test]
        fn task_xml_is_written_as_utf16le_with_bom() {
            let bytes = utf16le_with_bom("<T/>");
            assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
            assert_eq!(bytes.len(), 2 + 4 * 2);
            assert_eq!(&bytes[2..4], &[b'<', 0]);
        }
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
