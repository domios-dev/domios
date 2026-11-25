use super::{macos_path, run, xml_escape, Step};
use std::path::Path;

#[derive(Clone, Copy)]
pub struct MacosLaunchAgent<'a> {
    pub label: &'a str,
    pub bundle_id: &'a str,
    pub environment: &'a [(&'a str, &'a str)],
}

pub(super) fn validate_identifier(identifier: &str) -> Result<(), String> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err("Invalid macOS bundle identifier or launch agent label".into());
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
pub fn macos_bundle_identifier(app: &Path) -> Result<String, String> {
    let app = app.canonicalize().map_err(|error| error.to_string())?;
    let contents = app
        .parent()
        .filter(|parent| parent.ends_with("MacOS"))
        .and_then(Path::parent)
        .filter(|parent| {
            parent.ends_with("Contents")
                && parent
                    .parent()
                    .and_then(Path::extension)
                    .is_some_and(|ext| ext == "app")
        })
        .ok_or("Domios executable is outside a macOS application bundle")?;
    let info =
        plist::Value::from_file(contents.join("Info.plist")).map_err(|error| error.to_string())?;
    let identifier = info
        .as_dictionary()
        .and_then(|info| info.get("CFBundleIdentifier"))
        .and_then(plist::Value::as_string)
        .ok_or("macOS application bundle has no CFBundleIdentifier")?;
    validate_identifier(identifier)?;
    Ok(identifier.to_owned())
}

pub fn build_macos_plist(agent: MacosLaunchAgent<'_>, supervisor: &str, binary: &str) -> String {
    let label = xml_escape(agent.label);
    let bundle_id = xml_escape(agent.bundle_id);
    let supervisor = xml_escape(supervisor);
    let binary = xml_escape(binary);
    let environment = if agent.environment.is_empty() {
        String::new()
    } else {
        let entries = agent
            .environment
            .iter()
            .map(|(key, value)| {
                format!(
                    "        <key>{}</key><string>{}</string>\n",
                    xml_escape(key),
                    xml_escape(value)
                )
            })
            .collect::<String>();
        format!("    <key>EnvironmentVariables</key>\n    <dict>\n{entries}    </dict>\n")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>AssociatedBundleIdentifiers</key>
    <string>{bundle_id}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{supervisor}</string>
        <string>system</string>
        <string>supervise</string>
        <string>--binary</string>
        <string>{binary}</string>
    </array>
{environment}    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>30</integer>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

pub(super) fn install_steps(
    home: &Path,
    supervisor: &str,
    app: &str,
    agent: MacosLaunchAgent<'_>,
) -> Result<Vec<Step>, String> {
    validate_identifier(agent.label)?;
    validate_identifier(agent.bundle_id)?;
    let path = macos_path(home, agent.label);
    let bytes = build_macos_plist(agent, supervisor, app).into_bytes();
    let mut steps = vec![Step::WriteAtomic(path.clone(), bytes)];
    steps.extend(reload_steps(&path));
    Ok(steps)
}

fn reload_steps(path: &Path) -> [Step; 2] {
    let path = path.to_string_lossy();
    [
        run("launchctl", &["unload", &path], false),
        run("launchctl", &["load", &path], true),
    ]
}

#[cfg(target_os = "macos")]
pub fn refresh_macos_agent(
    home: &Path,
    label: &str,
    bundle_id: &str,
    app: &Path,
    supervised: bool,
) -> Result<(), String> {
    use super::{execute, MACOS_OPERATIONS};
    use std::io::ErrorKind;

    validate_identifier(label)?;
    validate_identifier(bundle_id)?;
    let _operation = MACOS_OPERATIONS.lock();
    let path = macos_path(home, label);
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    let existing = std::fs::read(&path).map_err(|error| error.to_string())?;
    let Some(updated) = refreshed_definition(&existing, label, bundle_id, &app.to_string_lossy())
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    // Unloading a supervisor that launched this client can kill the client with it.
    let reload = !supervised
        && std::process::Command::new("launchctl")
            .args(["list", label])
            .output()
            .map_err(|error| error.to_string())?
            .status
            .success();
    crate::atomic_write::write_atomic(&path, updated).map_err(|error| error.to_string())?;
    if reload {
        if let Err(error) = execute(reload_steps(&path).into()) {
            if let Err(restore_error) = crate::atomic_write::write_atomic(&path, existing) {
                return Err(format!(
                    "{error}; restoring the previous LaunchAgent failed: {restore_error}"
                ));
            }
            if let Err(restore_error) = execute(vec![run(
                "launchctl",
                &["load", &path.to_string_lossy()],
                true,
            )]) {
                return Err(format!(
                    "{error}; reloading the previous LaunchAgent failed: {restore_error}"
                ));
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn refreshed_definition(
    existing: &[u8],
    label: &str,
    bundle_id: &str,
    app: &str,
) -> Result<Option<Vec<u8>>, plist::Error> {
    use plist::Value;

    let Ok(mut definition) = Value::from_reader(std::io::Cursor::new(existing)) else {
        return Ok(None);
    };
    let Some(keys) = definition.as_dictionary_mut() else {
        return Ok(None);
    };
    let Some(arguments) = keys.get("ProgramArguments").and_then(Value::as_array) else {
        return Ok(None);
    };
    let [supervisor, system, supervise, binary_flag, binary] = arguments.as_slice() else {
        return Ok(None);
    };
    if keys.get("Label").and_then(Value::as_string) != Some(label)
        || !supervisor
            .as_string()
            .is_some_and(|path| path.starts_with('/'))
        || system.as_string() != Some("system")
        || supervise.as_string() != Some("supervise")
        || binary_flag.as_string() != Some("--binary")
        || binary.as_string() != Some(app)
    {
        return Ok(None);
    }
    let association = keys.get("AssociatedBundleIdentifiers");
    if association.and_then(Value::as_string) == Some(bundle_id)
        || association
            .and_then(Value::as_array)
            .is_some_and(|ids| matches!(ids.as_slice(), [id] if id.as_string() == Some(bundle_id)))
    {
        return Ok(None);
    }
    keys.insert("AssociatedBundleIdentifiers".into(), bundle_id.into());
    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, &definition)?;
    Ok(Some(bytes))
}
