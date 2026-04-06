//! Unified command dispatch: one `Command` variant per operation, one `dispatch()`
//! entry point. REST, Socket, Tauri IPC, and CLI adapters deserialize into Command
//! and call dispatch — no duplicated glue per transport.

use serde::{Deserialize, Serialize};
use crate::service::ServiceContext;

/// Unified command enum — every operation CodeTerm supports.
/// Start with 5 operations, migrate the rest incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Health check / system identify
    Health,

    /// List all panes with metadata
    PaneList,

    /// Send text to a pane's PTY
    SendText {
        pane_id: String,
        text: String,
        #[serde(default = "default_true")]
        enter: bool,
    },

    /// Get pane output (terminal buffer)
    GetOutput {
        pane_id: String,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        max_bytes: Option<usize>,
        #[serde(default)]
        strip_ansi: bool,
        #[serde(default)]
        full: bool,
    },

    /// Get pane screen state (VT100 parsed)
    GetScreen {
        pane_id: String,
        /// If true, return escape-coded bytes preserving colors/cursor
        #[serde(default)]
        formatted: bool,
    },
}

fn default_true() -> bool { true }

/// Unified result type for all commands.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CommandResult {
    Json(serde_json::Value),
}

impl CommandResult {
    pub fn into_json(self) -> serde_json::Value {
        match self {
            CommandResult::Json(v) => v,
        }
    }
}

/// Dispatch a command against ServiceContext. Pure logic, no transport concerns.
pub fn dispatch(cmd: &Command, ctx: &ServiceContext) -> Result<CommandResult, String> {
    match cmd {
        Command::Health => cmd_health(),
        Command::PaneList => cmd_pane_list(ctx),
        Command::SendText { pane_id, text, enter } => {
            cmd_send_text(ctx, pane_id, text, *enter)
        }
        Command::GetOutput { pane_id, offset, max_bytes, strip_ansi, full } => {
            cmd_get_output(ctx, pane_id, *offset, *max_bytes, *strip_ansi, *full)
        }
        Command::GetScreen { pane_id, formatted } => {
            cmd_get_screen(ctx, pane_id, *formatted)
        }
    }
}

// ── Command implementations ─────────────────────────────────────────

fn cmd_health() -> Result<CommandResult, String> {
    Ok(CommandResult::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("BUILD_HASH"),
        "build_number": env!("BUILD_NUMBER"),
        "name": "CodeTerm",
        "platform": std::env::consts::OS,
    })))
}

fn cmd_pane_list(ctx: &ServiceContext) -> Result<CommandResult, String> {
    let mux = ctx.mux.lock();
    let snapshot = mux.snapshot();
    let panes: Vec<serde_json::Value> = snapshot.panes.values()
        .map(|p| serde_json::to_value(p).unwrap_or_default())
        .collect();
    Ok(CommandResult::Json(serde_json::json!(panes)))
}

fn cmd_send_text(
    ctx: &ServiceContext,
    pane_id: &str,
    text: &str,
    enter: bool,
) -> Result<CommandResult, String> {
    ctx.send_text(pane_id, text, enter)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_get_output(
    ctx: &ServiceContext,
    pane_id: &str,
    offset: Option<usize>,
    max_bytes: Option<usize>,
    strip_ansi: bool,
    full: bool,
) -> Result<CommandResult, String> {
    let pty_id = ctx.resolve_pty_id(pane_id)?;
    let buf = ctx.output_buf.lock();
    let text = if full {
        buf.get_full_text(&pty_id).unwrap_or_default()
    } else {
        buf.get_text(&pty_id).unwrap_or_default()
    };

    let start = offset.unwrap_or(0);
    let raw = text.get(start..).unwrap_or("");

    // Apply max_bytes truncation (take last N bytes)
    let truncated = if let Some(max) = max_bytes {
        if raw.len() > max { &raw[raw.len() - max..] } else { raw }
    } else {
        raw
    };

    let output = if strip_ansi {
        crate::api::routes::clean_terminal_output(truncated)
    } else {
        truncated.to_string()
    };

    Ok(CommandResult::Json(serde_json::json!({
        "output": output,
        "offset": text.len(),
    })))
}

fn cmd_get_screen(
    ctx: &ServiceContext,
    pane_id: &str,
    formatted: bool,
) -> Result<CommandResult, String> {
    let pty_id = ctx.resolve_pty_id(pane_id)?;
    let ss = ctx.screen_state.lock();

    if formatted {
        let bytes = ss.get_screen_formatted(&pty_id)
            .ok_or_else(|| "No screen state".to_string())?;
        Ok(CommandResult::Json(serde_json::json!({
            "screen": bytes,
            "formatted": true,
        })))
    } else {
        let text = ss.get_screen_text(&pty_id).unwrap_or_default();
        Ok(CommandResult::Json(serde_json::json!({
            "screen": text.replace('\n', "\r\n"),
        })))
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_returns_ok() {
        let result = cmd_health().unwrap();
        let json = result.into_json();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["name"], "CodeTerm");
        assert!(json["version"].is_string());
        assert!(json["platform"].is_string());
    }

    #[test]
    fn test_command_deserialize_health() {
        let json = r#"{"cmd": "health"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::Health));
    }

    #[test]
    fn test_command_deserialize_send_text() {
        let json = r#"{"cmd": "send_text", "pane_id": "abc", "text": "hello"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::SendText { pane_id, text, enter } => {
                assert_eq!(pane_id, "abc");
                assert_eq!(text, "hello");
                assert!(enter); // default true
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_send_text_enter_false() {
        let json = r#"{"cmd": "send_text", "pane_id": "abc", "text": "hello", "enter": false}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::SendText { enter, .. } => assert!(!enter),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_get_output() {
        let json = r#"{"cmd": "get_output", "pane_id": "abc", "strip_ansi": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetOutput { pane_id, strip_ansi, full, offset, max_bytes } => {
                assert_eq!(pane_id, "abc");
                assert!(strip_ansi);
                assert!(!full); // default false
                assert!(offset.is_none());
                assert!(max_bytes.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_get_screen() {
        let json = r#"{"cmd": "get_screen", "pane_id": "abc", "formatted": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetScreen { pane_id, formatted } => {
                assert_eq!(pane_id, "abc");
                assert!(formatted);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_pane_list() {
        let json = r#"{"cmd": "pane_list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::PaneList));
    }

    #[test]
    fn test_command_result_into_json() {
        let result = CommandResult::Json(serde_json::json!({"test": true}));
        let json = result.into_json();
        assert_eq!(json["test"], true);
    }
}
