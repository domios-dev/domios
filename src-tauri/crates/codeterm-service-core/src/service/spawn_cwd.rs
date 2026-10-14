//! An agent runs where its tab says it runs. The PTY layer silently falls back
//! to the home directory for a cwd that is not a directory, so one is refused here.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CwdRefusal {
    pub path: String,
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
    pub problem: String,
    pub missing: bool,
}

impl CwdRefusal {
    pub const CODE: &'static str = "workspace_cwd_missing";
    pub const CLASS: &'static str = "requires_workspace_repair";

    pub fn message(&self) -> String {
        let subject = self
            .workspace_name
            .as_deref()
            .map(|name| format!("Workspace {name}"))
            .unwrap_or_else(|| "The requested working directory".into());
        let condition = if self.missing {
            "no longer exists".to_string()
        } else {
            format!("cannot be used ({})", self.problem)
        };
        format!("{subject} points to {}, which {condition}. Choose its new folder or recreate the workspace.", self.path)
    }

    pub fn data(&self) -> serde_json::Value {
        serde_json::json!({"code": Self::CODE, "class": Self::CLASS, "message": self.message(),
            "path": self.path, "workspace_id": self.workspace_id,
            "retry_hint": "Choose the workspace's new folder before retrying."})
    }
}

impl std::fmt::Display for CwdRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl From<CwdRefusal> for String {
    fn from(error: CwdRefusal) -> Self {
        error.message()
    }
}

#[cfg(test)]
#[path = "spawn_cwd/tests.rs"]
mod tests;

/// A spawn's requested cwd must be an absolute, existing directory on this host.
/// Relative paths are resolved by the caller, which alone knows its own directory.
pub(crate) fn validate_spawn_cwd(cwd: Option<&str>) -> Result<(), CwdRefusal> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.trim().is_empty()) else {
        return Ok(());
    };
    inspect_cwd(cwd).map_err(|(missing, problem)| CwdRefusal {
        path: cwd.into(),
        workspace_id: None,
        workspace_name: None,
        problem,
        missing,
    })
}

pub(crate) fn cwd_problem(cwd: &str) -> Option<String> {
    inspect_cwd(cwd).err().map(|(_, problem)| problem)
}

fn inspect_cwd(cwd: &str) -> Result<(), (bool, String)> {
    let path = Path::new(cwd);
    if !path.is_absolute() {
        return Err((
            false,
            format!(
                "'{cwd}' is not an absolute path on this host{}",
                foreign_path_hint(cwd)
            ),
        ));
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::read_dir(path.join("."))
            .map(|_| ())
            .map_err(|error| (false, format!("'{cwd}' cannot be read: {error}"))),
        Ok(_) => Err((false, format!("'{cwd}' is not a directory"))),
        Err(error) => Err((
            error.kind() == std::io::ErrorKind::NotFound,
            format!("'{cwd}' cannot be inspected: {error}"),
        )),
    }
}

fn foreign_path_hint(cwd: &str) -> &'static str {
    let bytes = cwd.as_bytes();
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if drive_path {
        "; this is a Windows path on a non-Windows host (Git Bash rewrites POSIX arguments unless MSYS_NO_PATHCONV=1 is set)"
    } else {
        "; pass an absolute directory"
    }
}

impl super::ServiceContext {
    pub(crate) fn validate_request_cwd(
        &self,
        workspace_id: Option<&str>,
        caller_tab_id: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(), CwdRefusal> {
        let workspace = {
            let mux = self.mux.read();
            let id = workspace_id
                .map(str::to_string)
                .or_else(|| caller_tab_id.and_then(|tab| mux.find_workspace_for_tab(tab)))
                .or_else(|| mux.get_active_workspace().map(|ws| ws.id.clone()))
                .or_else(|| mux.list_workspaces().first().map(|ws| ws.id.clone()));
            id.and_then(|id| mux.get_workspace(&id).cloned())
        };
        let path = cwd.or_else(|| workspace.as_ref().and_then(|ws| ws.cwd.as_deref()));
        if let Err(mut error) = validate_spawn_cwd(path) {
            error.workspace_id = workspace.as_ref().map(|ws| ws.id.clone());
            error.workspace_name = workspace.as_ref().map(|ws| ws.name.clone());
            if let Some(ws) = workspace.as_ref() {
                let _ = self.refresh_workspace_cwd_state(&ws.id);
            }
            log::warn!(
                "[spawn] cwd refused: tab={:?} workspace={:?} path={} reason={}",
                caller_tab_id,
                error.workspace_id,
                error.path,
                error.problem
            );
            return Err(error);
        }
        Ok(())
    }
}
