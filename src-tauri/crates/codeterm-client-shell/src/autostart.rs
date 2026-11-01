//! Native Settings adapter for the shared supervisor registration.
pub fn is_enabled() -> bool {
    dirs::home_dir().is_some_and(|home| codeterm_util::autostart::is_enabled(&home))
}

pub fn enable() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    let app = std::env::current_exe().map_err(|error| error.to_string())?;
    codeterm_util::autostart::enable(&home, &codeterm_paths::data_dir(), &cli_path()?, &app)
}

pub fn disable() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    codeterm_util::autostart::disable(&home, &codeterm_paths::data_dir())
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
