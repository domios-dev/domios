use crate::app_runtime::AppHandle;
use tauri::Manager;

pub(crate) fn restart(app: &AppHandle) -> Result<(), String> {
    let executable =
        tauri::process::current_binary(&app.env()).map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(&executable);
    #[cfg(target_os = "macos")]
    if let Some(bundle) = executable
        .parent()
        .and_then(|path| path.parent())
        .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
        .and_then(|path| path.parent())
    {
        command = std::process::Command::new("/usr/bin/open");
        command.arg("-n").arg(bundle).arg("--args");
    }
    command.args(std::env::args_os().skip(1));
    codeterm_paths::relaunch_env::apply_relaunch_env(&mut command);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    codeterm_util::process_util::no_window_std(&mut command);
    app.cleanup_before_exit();
    command
        .spawn()
        .map_err(|error| format!("failed to relaunch application: {error}"))?;
    std::process::exit(0)
}
