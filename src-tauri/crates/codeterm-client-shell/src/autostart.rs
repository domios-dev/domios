//! Native Settings adapter for the shared supervisor registration.
use crate::app_runtime::AppHandle;
use codeterm_util::autostart;

pub fn is_enabled(app: &AppHandle) -> bool {
    dirs::home_dir().is_some_and(|home| autostart::is_enabled(&home, &app.config().identifier))
}

pub fn enable(handle: &AppHandle) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    let app = std::env::current_exe().map_err(|error| error.to_string())?;
    let data = codeterm_paths::data_dir();
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;

        let label = &handle.config().identifier;
        let bundle_id = autostart::macos_bundle_identifier(&app)?;
        let relaunch = handle.state::<codeterm_paths::relaunch_env::ApplicationRelaunchEnv>();
        let mut environment = Vec::new();
        let cli = if label == autostart::MACOS_LABEL {
            cli_path()?
        } else {
            environment.push((
                "CODETERM_DATA_DIR",
                data.to_str().ok_or("Invalid DEV data path")?,
            ));
            environment.extend(
                [
                    "CODETERM_DEV_INSTANCE",
                    "CODETERM_DEV_API_PORT",
                    "CODETERM_DEV_IDENTIFIER",
                    "CODETERM_VITE_PORT",
                    "CODETERM_CDP_PORT",
                ]
                .into_iter()
                .filter_map(|key| relaunch.inner().get(key).map(|value| (key, value))),
            );
            let cli = app
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("Invalid DEV application path")?
                .join("Resources/cli/codeterm");
            if !cli.is_file() {
                return Err("Bundled DEV supervisor is missing".into());
            }
            cli
        };
        autostart::enable(
            &home,
            &data,
            &cli,
            &app,
            Some(autostart::MacosLaunchAgent {
                label,
                bundle_id: &bundle_id,
                environment: &environment,
            }),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = handle;
        autostart::enable(&home, &data, &cli_path()?, &app, None)
    }
}

pub fn disable(app: &AppHandle) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    autostart::disable(&home, &codeterm_paths::data_dir(), &app.config().identifier)
}

#[cfg(target_os = "macos")]
pub fn refresh_owned_macos_agent(label: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    if !autostart::is_enabled(&home, label) {
        return Ok(());
    }
    let app = std::env::current_exe().map_err(|error| error.to_string())?;
    let bundle_id = autostart::macos_bundle_identifier(&app)?;
    let supervised = std::env::args_os().any(|arg| arg == "--supervised");
    autostart::refresh_macos_agent(&home, label, &bundle_id, &app, supervised)
}

#[cfg(target_os = "linux")]
pub fn refresh_owned_linux_unit() -> Result<(), String> {
    if std::env::var_os("DOMIOS_UPDATE_ROLLBACK").is_some() {
        return Ok(());
    }
    let home = dirs::home_dir().ok_or("home dir not found")?;
    let path = codeterm_util::autostart::linux_path(&home);
    let owned = std::fs::read_to_string(path).is_ok_and(|unit| {
        unit.lines()
            .any(|line| line == codeterm_util::autostart::LINUX_OWNERSHIP_MARKER)
    });
    if !owned {
        return Ok(());
    }
    codeterm_util::autostart::refresh_linux_unit(
        &home,
        &cli_path()?,
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )
}

fn cli_path() -> Result<std::path::PathBuf, String> {
    let name = if cfg!(windows) {
        "domios.exe"
    } else {
        "domios"
    };
    let home = dirs::home_dir().ok_or_else(|| "home dir not found".to_string())?;
    for candidate in [
        home.join(".domios").join("bin").join(name),
        home.join(".domios-dev").join("bin").join(name),
    ] {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("domios CLI is not installed yet; restart Domios and try again".into())
}
