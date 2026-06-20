//! Plugin-management logic shared by the `codeterm` CLI and the daemon.
//!
//! This crate owns the filesystem-level plugin/channel operations (list,
//! install, update, channel add/refresh/diff/remove, get/config). Both the CLI
//! binary and the daemon call [`run`] in-process so the daemon never forks its
//! own CLI to manage plugins (AGENTS.md: "The daemon NEVER shells out to its
//! own `codeterm` CLI"). The only plugin path that still needs a socket — the
//! `plugin <id> <verb>` passthrough — stays in the CLI crate, since it routes
//! to a running daemon rather than touching the filesystem.

use std::cell::RefCell;
use std::path::PathBuf;

// Shared glob surface for the submodules. They are written against `super::*`,
// so the crate root must re-export the same names the old `cmd::plugin` facade
// did (helpers, types, consts) plus the std/sha2 imports they rely on.
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;

mod audit;
mod channel;
mod config;
mod install;
mod list;
mod state;
mod types;
mod update;
mod util;
mod version;

#[cfg(test)]
mod tests;

use audit::*;
use channel::*;
use install::*;
use list::*;
use state::*;
use types::*;
use update::*;
use util::*;
use version::*;

/// Runtime context for a plugin operation: where plugin data lives and which
/// CodeTerm version is running. Callers pass this explicitly so the logic does
/// not depend on a particular process's env layout (the daemon resolves its own
/// data dir; the CLI resolves dev-vs-prod) while keeping behaviour identical to
/// the previous subprocess, which received both via `CODETERM_DATA_DIR` /
/// `CODETERM_VERSION`.
#[derive(Clone, Debug)]
pub struct PluginCtx {
    pub data_root: PathBuf,
    pub version: Option<String>,
}

thread_local! {
    static CTX: RefCell<Option<PluginCtx>> = const { RefCell::new(None) };
}

/// Restores the previous context on drop so nested/re-entrant calls and pooled
/// `spawn_blocking` threads never leak a stale data root.
struct CtxGuard(Option<PluginCtx>);

impl Drop for CtxGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        CTX.with(|c| *c.borrow_mut() = prev);
    }
}

fn enter_ctx(ctx: PluginCtx) -> CtxGuard {
    CTX.with(|c| {
        let prev = c.borrow_mut().replace(ctx);
        CtxGuard(prev)
    })
}

/// Plugin data root. Prefers the active [`PluginCtx`]; falls back to the
/// `CODETERM_DATA_DIR` env override (must be absolute) and finally to
/// `~/.codeterm`. The fallback path is exercised only by tests (which set the
/// env) — real callers always establish a context first.
pub(crate) fn data_root() -> PathBuf {
    if let Some(root) = CTX.with(|c| c.borrow().as_ref().map(|x| x.data_root.clone())) {
        return root;
    }
    if let Ok(root) = std::env::var("CODETERM_DATA_DIR") {
        if !root.trim().is_empty() {
            let path = PathBuf::from(root);
            if path.is_absolute() {
                return path;
            }
        }
    }
    dirs::home_dir().unwrap_or_default().join(".codeterm")
}

/// Running CodeTerm version. Prefers the active [`PluginCtx`]; falls back to the
/// `CODETERM_VERSION` env var (tests, or a raw CLI invocation). `None` makes
/// `compat_check` fail-open.
pub(crate) fn ctx_version() -> Option<String> {
    CTX.with(|c| c.borrow().as_ref().and_then(|x| x.version.clone()))
        .or_else(|| {
            std::env::var("CODETERM_VERSION")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
}

/// Plugin-management action. Mirrors the CLI's clap `PluginAction` minus the
/// clap derive so the daemon can build it without depending on the CLI crate.
/// `Pass` is kept for failure auditing only — [`run`] never executes it
/// (the passthrough lives in the CLI, where the daemon socket is reachable).
#[derive(Clone, Debug)]
pub enum PluginAction {
    List,
    Install {
        id: Option<String>,
        from_dir: Option<String>,
        id_override: Option<String>,
        channel: Option<String>,
    },
    Update {
        id: Option<String>,
    },
    UpdateSystem {
        id: Option<String>,
    },
    Remove {
        id: String,
        force: bool,
        remove_data: bool,
    },
    Channel(PluginChannelAction),
    Get {
        id: String,
    },
    Config {
        id: String,
        set: Vec<String>,
        values_json: Option<String>,
        grant: Vec<String>,
        revoke: Vec<String>,
    },
    Pass(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum PluginChannelAction {
    Add { url: String, name: Option<String> },
    EnsureDefault,
    List,
    Refresh { name: Option<String> },
    Diff { name: Option<String> },
    Remove { name: String, force: bool },
}

/// Execute a plugin-management action in-process and return the JSON value the
/// CLI would have printed with `--json`. On error, records the same
/// command-failure audit event the CLI does, then propagates the error.
pub fn run(ctx: PluginCtx, action: PluginAction) -> Result<serde_json::Value, String> {
    let _guard = enter_ctx(ctx);
    let result = dispatch(action.clone());
    if let Err(e) = &result {
        let _ = audit_command_failure(&action, e);
    }
    result
}

fn dispatch(action: PluginAction) -> Result<serde_json::Value, String> {
    match action {
        PluginAction::List => list_installed(),
        PluginAction::Install {
            id,
            from_dir,
            id_override,
            channel,
        } => install_command(id, from_dir, id_override, channel),
        PluginAction::Update { id } => update_command(id),
        PluginAction::UpdateSystem { id } => update_system_command(id),
        PluginAction::Remove {
            id,
            force,
            remove_data,
        } => remove_installed(&id, force, remove_data),
        PluginAction::Channel(command) => channel_command(command),
        PluginAction::Get { id } => config::get_command(&id),
        PluginAction::Config {
            id,
            set,
            values_json,
            grant,
            revoke,
        } => config::config_command(&id, &set, values_json.as_deref(), &grant, &revoke),
        PluginAction::Pass(_) => Err("plugin passthrough is not available in-process".to_string()),
    }
}

/// Record a command-failure audit event for an action that failed outside
/// [`run`] (the CLI's socket passthrough). Establishes a context so the audit
/// log lands under the right data root.
pub fn audit_failure(ctx: PluginCtx, action: &PluginAction, error: &str) {
    let _guard = enter_ctx(ctx);
    let _ = audit_command_failure(action, error);
}

#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
