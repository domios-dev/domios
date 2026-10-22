use crate::app_runtime::AppHandle;
use tauri::Manager;

pub(crate) fn restart(app: &AppHandle) -> Result<(), String> {
    let executable =
        tauri::process::current_binary(&app.env()).map_err(|error| error.to_string())?;
    let mut command = relaunch_command(&executable);
    command.args(std::env::args_os().skip(1));
    app.try_state::<codeterm_paths::relaunch_env::ApplicationRelaunchEnv>()
        .ok_or("application relaunch environment is unavailable")?
        .apply(&mut command);
    codeterm_util::process_util::no_window_std(&mut command);
    #[cfg(target_os = "macos")]
    release_single_instance(app);
    app.cleanup_before_exit();
    command
        .spawn()
        .map_err(|error| format!("failed to relaunch application: {error}"))?;
    std::process::exit(0)
}

#[cfg(target_os = "macos")]
fn release_single_instance(app: &AppHandle) {
    tauri_plugin_single_instance::destroy(app);
}

fn relaunch_command(executable: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

#[cfg(test)]
mod tests;
