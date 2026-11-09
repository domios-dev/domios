use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

pub const FILE_ENV: &str = "CODETERM_LAUNCH_FILE";
pub const RUNTIME_FILE_PREFIX: &str = ".runtime-launch-";
pub const HANDOFF_FILE_PREFIX: &str = ".launch-credential-";
/// Set only for a runtime that holds input behind a running turn. A hint, never authority.
/// Agent harnesses strip credential-shaped names from tool shells, so this name must not look like one.
pub const RUNTIME_AGENT_ENV: &str = "CODETERM_RUNTIME_AGENT";
pub const RESTART_GUIDANCE: &str = "This tab has no valid launch credential; restart the tab";
pub const MARKERS: &[&str] = &[
    "CODETERM",
    "CODETERM_TAB_ID",
    "CODETERM_CLI_SCOPE",
    "CODETERM_SESSION_ID",
    FILE_ENV,
    "CODETERM_SESSION_BINDING_NONCE",
    "CODETERM_SHELL_NONCE",
    "CODETERM_AGENT_SHELL",
    "CODETERM_ROLE",
    "CODETERM_TEAM_ID",
    "CODETERM_PLAN_ID",
    "CODETERM_PARENT_TAB_ID",
    "CODETERM_WORKER_ID",
    "CODETERM_TASK_ID",
    "CODETERM_ATTEMPT_ID",
    "CODETERM_LANE_BASE",
    "CODETERM_STARTUP_ATTEMPT_ID",
    "CODETERM_PAD_DIR",
];

pub fn marked() -> bool {
    MARKERS.iter().any(|name| std::env::var_os(name).is_some())
}

pub fn lookup(env: impl Fn(&str) -> Option<String>) -> Result<Option<String>, String> {
    if !MARKERS.iter().any(|name| env(name).is_some()) {
        return Ok(None);
    }
    let path = env(FILE_ENV);
    resolve(
        env("CODETERM_API_TOKEN").as_deref(),
        path.as_deref().map(Path::new),
        env("CODETERM_TAB_ID").as_deref(),
        env("CODETERM_SESSION_BINDING_NONCE").as_deref(),
    )
    .map(Some)
}

#[derive(Serialize, Deserialize)]
pub struct LaunchCredential {
    pub tab_id: String,
    pub binding_nonce: String,
    pub token: String,
}

pub fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = crate::private_file::create_new_owner_only(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn resolve(
    environment: Option<&str>,
    path: Option<&Path>,
    tab: Option<&str>,
    nonce: Option<&str>,
) -> Result<String, String> {
    if let Some(token) = environment.filter(|token| !token.trim().is_empty()) {
        return Ok(token.to_owned());
    }
    let read = || -> Option<String> {
        let path = path?;
        let mut bytes = Vec::new();
        crate::private_file::open_owner_only(path, 8192)
            .ok()?
            .take(8193)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 8192 {
            return None;
        }
        let credential: LaunchCredential = serde_json::from_slice(&bytes).ok()?;
        (tab == Some(credential.tab_id.as_str())
            && nonce == Some(credential.binding_nonce.as_str())
            && !credential.tab_id.is_empty()
            && !credential.binding_nonce.is_empty()
            && !credential.token.trim().is_empty())
        .then_some(credential.token)
    };
    read().ok_or_else(|| RESTART_GUIDANCE.to_owned())
}

#[cfg(test)]
mod tests;
