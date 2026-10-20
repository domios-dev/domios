use super::*;

const WRAPPER_MARKER: &str = "codeterm-keepawake-wrapper";
const GNOME_SESSION_SCHEMA: &str = "org.gnome.desktop.session";
const GNOME_POWER_SCHEMA: &str = "org.gnome.settings-daemon.plugins.power";
const SYSTEMD_TARGETS: &[&str] = &[
    "sleep.target",
    "suspend.target",
    "hibernate.target",
    "hybrid-sleep.target",
];
const SYSTEMD_SERVICES: &[&str] = &[
    "systemd-suspend.service",
    "systemd-hibernate.service",
    "systemd-hybrid-sleep.service",
    "systemd-suspend-then-hibernate.service",
];

pub fn start<E: StatusEmitter>(
    display: bool,
    duration_secs: Option<u64>,
    stop_rx: std::sync::mpsc::Receiver<()>,
    state: KeepAwakeState,
    emitter: E,
    session_id: u64,
) -> Result<(), String> {
    let mut child = spawn_keepawake_child(display)?;
    let child_pid = child.id();
    register_child(child_pid, display, true);

    std::thread::Builder::new()
        .name("keepawake".into())
        .spawn(move || {
            let stopped_early = match duration_secs {
                Some(secs) => stop_rx
                    .recv_timeout(std::time::Duration::from_secs(secs))
                    .is_ok(),
                None => stop_rx.recv().is_ok(),
            };

            let _ = terminate_pid(child_pid);
            let _ = child.wait();
            unregister_child(child_pid);

            if !stopped_early {
                clear_if_owner(&state, session_id, &emitter);
            }
        })
        .map(|_| ())
        .map_err(|e| {
            let _ = terminate_pid(child_pid);
            unregister_child(child_pid);
            format!("failed to spawn keepawake thread: {}", e)
        })
}

fn spawn_keepawake_child(display: bool) -> Result<std::process::Child, String> {
    let available = std::process::Command::new("systemd-inhibit")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !available {
        return Err(
            "systemd-inhibit is required for Linux keepawake runtime inhibition".to_string(),
        );
    }
    let script = keepawake_wrapper_script(display);
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(script).arg(WRAPPER_MARKER);
    cmd.spawn()
        .map_err(|e| format!("systemd-inhibit wrapper spawn failed: {}", e))
}

fn keepawake_wrapper_script(display: bool) -> String {
    let what = if display { "sleep:idle" } else { "sleep" };
    format!(
        r#"
if ! command -v systemd-inhibit >/dev/null 2>&1; then
  echo "systemd-inhibit not found" >&2
  exit 127
fi
exec systemd-inhibit --what={what} --mode=block --who=CodeTerm --why="CodeTerm keepawake session" sleep infinity
"#
    )
}

pub fn diagnostic(active: bool, display: bool) -> Option<KeepAwakeDiagnostic> {
    let inhibitors = std::process::Command::new("systemd-inhibit")
        .arg("--list")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
        .unwrap_or_default();
    let runtime_inhibit_active = parse_systemd_inhibit_list(&inhibitors, "sleep");
    let display_inhibit_active = parse_systemd_inhibit_list(&inhibitors, "idle");

    let gnome_settings = read_gnome_settings();
    let unsafe_gnome_settings = unsafe_gnome_settings(&gnome_settings);
    let gnome_policy_safe = if gnome_settings.is_empty() {
        None
    } else {
        Some(unsafe_gnome_settings.is_empty())
    };

    let masked = read_systemd_masks();
    let missing_systemd_masks = missing_systemd_masks(&masked);
    let systemd_hard_block_active = if masked.is_empty() {
        None
    } else {
        Some(missing_systemd_masks.is_empty())
    };
    let systemd_daemon_reload_needed = systemd_daemon_reload_needed();

    let mut diagnostic = KeepAwakeDiagnostic {
        runtime_inhibit_active,
        display_inhibit_active,
        gnome_policy_safe,
        systemd_hard_block_active,
        systemd_daemon_reload_needed,
        unsafe_gnome_settings,
        missing_systemd_masks,
    };
    if !active {
        diagnostic.runtime_inhibit_active = false;
        diagnostic.display_inhibit_active = false;
    }
    if !display {
        diagnostic.display_inhibit_active = false;
    }
    Some(diagnostic)
}

pub fn apply_user_power_policy() -> Result<(), String> {
    let settings = [
        (GNOME_POWER_SCHEMA, "sleep-inactive-ac-type", "nothing"),
        (GNOME_POWER_SCHEMA, "sleep-inactive-battery-type", "nothing"),
        (GNOME_POWER_SCHEMA, "lid-close-ac-action", "nothing"),
        (GNOME_POWER_SCHEMA, "lid-close-battery-action", "nothing"),
        (GNOME_POWER_SCHEMA, "power-button-action", "nothing"),
        (GNOME_SESSION_SCHEMA, "idle-delay", "0"),
    ];
    for (schema, key, value) in settings {
        set_gsettings(schema, key, value)?;
    }
    Ok(())
}

pub fn system_block_commands() -> Vec<String> {
    vec![
        "sudo systemctl daemon-reload".to_string(),
        format!("sudo systemctl mask {}", SYSTEMD_TARGETS.join(" ")),
        format!("sudo systemctl mask {}", SYSTEMD_SERVICES.join(" ")),
    ]
}

fn read_gnome_settings() -> Vec<(String, String, String)> {
    let keys = [
        (GNOME_POWER_SCHEMA, "sleep-inactive-ac-type"),
        (GNOME_POWER_SCHEMA, "sleep-inactive-battery-type"),
        (GNOME_POWER_SCHEMA, "lid-close-ac-action"),
        (GNOME_POWER_SCHEMA, "lid-close-battery-action"),
        (GNOME_POWER_SCHEMA, "power-button-action"),
        (GNOME_SESSION_SCHEMA, "idle-delay"),
    ];
    keys.into_iter()
        .filter_map(|(schema, key)| {
            let output = std::process::Command::new("gsettings")
                .args(["get", schema, key])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            Some((
                schema.to_string(),
                key.to_string(),
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        })
        .collect()
}

fn set_gsettings(schema: &str, key: &str, value: &str) -> Result<(), String> {
    let mut cmd = std::process::Command::new("gsettings");
    cmd.args(["set", schema, key, value]);
    let out = cmd
        .output()
        .map_err(|e| format!("gsettings set {} {} failed to start: {}", schema, key, e))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!(
            "gsettings set {} {} failed: {}",
            schema,
            key,
            if stderr.is_empty() {
                out.status.to_string()
            } else {
                stderr
            }
        ))
    }
}

fn read_systemd_masks() -> Vec<(String, String)> {
    SYSTEMD_TARGETS
        .iter()
        .chain(SYSTEMD_SERVICES.iter())
        .filter_map(|unit| {
            let output = std::process::Command::new("systemctl")
                .args(["is-enabled", unit])
                .output()
                .ok()?;
            Some((
                (*unit).to_string(),
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        })
        .collect()
}

fn systemd_daemon_reload_needed() -> Option<bool> {
    let output = std::process::Command::new("systemctl")
        .args(["show", "-p", "NeedDaemonReload", "--value"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_systemd_daemon_reload_needed(
        &String::from_utf8_lossy(&output.stdout),
    ))
}

pub(crate) fn parse_systemd_inhibit_list(output: &str, what: &str) -> bool {
    output
        .lines()
        .filter(|line| line.to_ascii_lowercase().contains("codeterm"))
        .any(|line| {
            line.split_whitespace()
                .any(|part| part.split(':').any(|token| token == what))
        })
}

pub(crate) fn unsafe_gnome_settings(settings: &[(String, String, String)]) -> Vec<String> {
    settings
        .iter()
        .filter_map(|(schema, key, value)| {
            let safe = match key.as_str() {
                "sleep-inactive-ac-type"
                | "sleep-inactive-battery-type"
                | "lid-close-ac-action"
                | "lid-close-battery-action"
                | "power-button-action" => value.trim_matches('\'') == "nothing",
                "idle-delay" => {
                    let raw = value.strip_prefix("uint32 ").unwrap_or(value);
                    raw.trim() == "0"
                }
                _ => true,
            };
            (!safe).then(|| format!("{}.{}={}", schema, key, value))
        })
        .collect()
}

pub(crate) fn missing_systemd_masks(statuses: &[(String, String)]) -> Vec<String> {
    statuses
        .iter()
        .filter(|(_, status)| status != "masked")
        .map(|(unit, _)| unit.clone())
        .collect()
}

pub(crate) fn parse_systemd_daemon_reload_needed(output: &str) -> bool {
    matches!(
        output.trim().to_ascii_lowercase().as_str(),
        "yes" | "true" | "1"
    )
}
