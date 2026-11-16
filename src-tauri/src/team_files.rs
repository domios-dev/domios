//! A double-clicked `.team` package reaches Domios as a launch argument, a forwarded second
//! launch or a macOS Opened URL; the newest one waits here until the frontend takes it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// Wakes the frontend to take the waiting package; the path travels only through the command.
pub(crate) const TEAM_FILE_OPENED_EVENT: &str = "team-file-opened";

#[derive(Default)]
pub(crate) struct OpenedTeamFile(Mutex<Option<PathBuf>>);

impl OpenedTeamFile {
    fn replace(&self, path: PathBuf) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path);
    }

    fn take(&self) -> Option<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

/// The package the OS opened Domios with, taken once; the frontend imports it and shows its consent review.
#[tauri::command]
pub(crate) fn take_opened_team_file(state: tauri::State<'_, OpenedTeamFile>) -> Option<String> {
    state
        .take()
        .and_then(|path| path.to_str().map(str::to_owned))
}

/// First launch: the window is not up yet, so the package only waits for the frontend.
pub(crate) fn receive_launch_arguments(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .filter_map(|argument| argument.into_string().ok())
        .collect();
    let cwd = std::env::current_dir().ok();
    receive(
        app,
        arguments.iter().map(String::as_str),
        cwd.as_deref(),
        false,
    );
}

/// A second launch forwarded by single-instance; its relative paths resolve against its own cwd.
pub(crate) fn receive_forwarded_launch(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    arguments: &[String],
    cwd: &str,
) -> bool {
    let cwd = (!cwd.is_empty()).then(|| Path::new(cwd));
    receive(app, arguments.iter().map(String::as_str), cwd, true)
}

#[cfg(target_os = "macos")]
pub(crate) fn receive_opened_urls(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    urls: &[url::Url],
) {
    receive(app, urls.iter().map(url::Url::as_str), None, true);
}

fn receive<'a>(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    arguments: impl IntoIterator<Item = &'a str>,
    cwd: Option<&Path>,
    activate: bool,
) -> bool {
    let Some(path) = opened_team_file(arguments, cwd, Path::is_file) else {
        return false;
    };
    let Some(slot) = app.try_state::<OpenedTeamFile>() else {
        return false;
    };
    slot.replace(path);
    if let Err(error) = app.emit(TEAM_FILE_OPENED_EVENT, ()) {
        log::debug!("[team-file] frontend wake failed: {error}");
    }
    if activate {
        crate::startup::desktop_lifecycle::activate(app);
    }
    true
}

/// The newest existing `.team` package among launch arguments; the executable itself never matches.
fn opened_team_file<'a>(
    arguments: impl IntoIterator<Item = &'a str>,
    cwd: Option<&Path>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    arguments
        .into_iter()
        .filter_map(|argument| team_file_argument(argument, cwd))
        .filter(|path| is_file(path))
        .last()
}

/// A `.team` archive path or file URL; `name.team.yaml` and every other file type stay unclaimed.
fn team_file_argument(argument: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    if argument.is_empty() || argument.starts_with('-') {
        return None;
    }
    let path = if has_file_scheme(argument) {
        url::Url::parse(argument).ok()?.to_file_path().ok()?
    } else {
        PathBuf::from(argument)
    };
    let team = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("team"));
    if !team {
        return None;
    }
    Some(match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    })
}

fn has_file_scheme(argument: &str) -> bool {
    argument
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
}

#[cfg(test)]
mod tests;
