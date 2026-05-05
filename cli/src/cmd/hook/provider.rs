//! Provider-shape normaliser. Hooks fire from three different agent CLIs
//! that don't agree on field names. We absorb the differences here so the
//! per-event handlers see one common `HookCtx` and don't care who called.
//!
//! Wire formats (Phase 2 supports — Phase 3 installer pins these):
//!
//! Claude Code (`tool_name`/`tool_input` shape, `hook_event_name` snake_case):
//! ```json
//! { "session_id": "…", "hook_event_name": "PreToolUse",
//!   "tool_name": "Bash",
//!   "tool_input": { "command": "cargo test", "description": "…" } }
//! ```
//!
//! OpenCode (`tool`/`args`, lowercase tool names — provisional, finalised
//! by the Phase 3 JS plugin shim):
//! ```json
//! { "tool": "bash", "args": { "command": "cargo test" } }
//! ```
//!
//! Codex (deferred — same shape as Claude until proven otherwise).
//!
//! Generic (`{"command": "…"}` flat shape — for ad-hoc hook smoke tests
//! that don't want to mimic any vendor).

use crate::args::hook::HookProvider;
use serde_json::Value;

/// Normalised pre-tool-use context — common shape all per-event handlers
/// see, regardless of which provider fired the hook.
#[derive(Debug, Clone)]
pub struct PreToolUseCtx {
    pub provider: HookProvider,
    pub tool_name: String,
    pub command: Option<String>,
    /// Original parsed JSON kept verbatim so the writer can splice the
    /// rewritten command back into the provider's expected response shape
    /// without losing other fields.
    pub raw: Value,
}

/// Parse the raw stdin JSON into a `PreToolUseCtx`. Returns `None` when
/// the JSON is shaped wrong for this event (the caller should pass
/// through unmodified — never block the tool).
pub fn parse_pre_tool_use(provider: HookProvider, input: &str) -> Option<PreToolUseCtx> {
    let raw: Value = serde_json::from_str(input).ok()?;
    let (tool_name, command) = match provider {
        HookProvider::Claude | HookProvider::Codex => {
            let tool_name = raw.get("tool_name").and_then(Value::as_str)?.to_string();
            let command = raw
                .get("tool_input")
                .and_then(|v| v.get("command"))
                .and_then(Value::as_str)
                .map(str::to_string);
            (tool_name, command)
        }
        HookProvider::Opencode => {
            let tool_name = raw.get("tool").and_then(Value::as_str)?.to_string();
            let command = raw
                .get("args")
                .and_then(|v| v.get("command"))
                .and_then(Value::as_str)
                .map(str::to_string);
            (tool_name, command)
        }
        HookProvider::Generic => {
            let tool_name = raw
                .get("tool")
                .or_else(|| raw.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("Bash")
                .to_string();
            let command = raw
                .get("command")
                .or_else(|| raw.get("tool_input").and_then(|v| v.get("command")))
                .and_then(Value::as_str)
                .map(str::to_string);
            (tool_name, command)
        }
    };
    Some(PreToolUseCtx {
        provider,
        tool_name,
        command,
        raw,
    })
}

/// True if this tool invocation is a Bash command we should consider
/// wrapping. Each provider names the bash tool differently.
pub fn is_bash(ctx: &PreToolUseCtx) -> bool {
    matches!(ctx.tool_name.as_str(), "Bash" | "bash" | "shell" | "Shell")
}

/// Build the response JSON the provider expects after we've decided to
/// rewrite the command. For Claude this is the `hookSpecificOutput` shape
/// with `updatedInput`; for OpenCode and Generic we return a flatter
/// shape that the Phase 3 JS shim translates back into a plugin
/// mutation. `new_command` is the wrapped string.
pub fn render_rewrite(ctx: &PreToolUseCtx, new_command: &str) -> String {
    match ctx.provider {
        HookProvider::Claude | HookProvider::Codex => {
            let mut updated_input = ctx
                .raw
                .get("tool_input")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = updated_input.as_object_mut() {
                obj.insert("command".into(), Value::String(new_command.to_string()));
            }
            serde_json::to_string(&serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "updatedInput": updated_input,
                }
            }))
            .unwrap_or_default()
        }
        HookProvider::Opencode => {
            let mut updated_args = ctx
                .raw
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = updated_args.as_object_mut() {
                obj.insert("command".into(), Value::String(new_command.to_string()));
            }
            serde_json::to_string(&serde_json::json!({
                "tool": ctx.tool_name,
                "args": updated_args,
            }))
            .unwrap_or_default()
        }
        HookProvider::Generic => serde_json::to_string(&serde_json::json!({
            "command": new_command,
        }))
        .unwrap_or_default(),
    }
}

/// Empty-output marker — what we emit when an event has no opinion. For
/// most providers an empty stdout means "use the input unchanged."
pub fn render_passthrough() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claude_bash() {
        let json = r#"{
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test", "description": "run tests" }
        }"#;
        let ctx = parse_pre_tool_use(HookProvider::Claude, json).expect("parses");
        assert_eq!(ctx.tool_name, "Bash");
        assert_eq!(ctx.command.as_deref(), Some("cargo test"));
        assert!(is_bash(&ctx));
    }

    #[test]
    fn parse_claude_non_bash_returns_no_command() {
        let json = r#"{ "tool_name": "Read", "tool_input": { "path": "/tmp/x" } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Claude, json).expect("parses");
        assert_eq!(ctx.tool_name, "Read");
        assert_eq!(ctx.command, None);
        assert!(!is_bash(&ctx));
    }

    #[test]
    fn parse_opencode_bash() {
        let json = r#"{ "tool": "bash", "args": { "command": "go test ./..." } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Opencode, json).expect("parses");
        assert_eq!(ctx.tool_name, "bash");
        assert_eq!(ctx.command.as_deref(), Some("go test ./..."));
        assert!(is_bash(&ctx));
    }

    #[test]
    fn parse_generic_flat_shape() {
        let json = r#"{ "command": "pytest -v" }"#;
        let ctx = parse_pre_tool_use(HookProvider::Generic, json).expect("parses");
        assert_eq!(ctx.command.as_deref(), Some("pytest -v"));
    }

    #[test]
    fn parse_generic_nested_shape() {
        let json = r#"{ "tool_name": "Bash", "tool_input": { "command": "npm test" } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Generic, json).expect("parses");
        assert_eq!(ctx.command.as_deref(), Some("npm test"));
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_pre_tool_use(HookProvider::Claude, "not json").is_none());
    }

    #[test]
    fn parse_empty_object_claude_returns_none() {
        assert!(parse_pre_tool_use(HookProvider::Claude, "{}").is_none());
    }

    #[test]
    fn parse_missing_command_field_keeps_tool_name() {
        let json = r#"{ "tool_name": "Bash", "tool_input": {} }"#;
        let ctx = parse_pre_tool_use(HookProvider::Claude, json).expect("parses");
        assert_eq!(ctx.tool_name, "Bash");
        assert_eq!(ctx.command, None);
    }

    #[test]
    fn render_claude_uses_hook_specific_output_shape() {
        let json = r#"{ "tool_name": "Bash", "tool_input": { "command": "cargo test" } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Claude, json).unwrap();
        let out = render_rewrite(&ctx, "codeterm snip cargo test");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["hookSpecificOutput"]["hookEventName"]
                .as_str()
                .unwrap(),
            "PreToolUse"
        );
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow"
        );
        assert_eq!(
            v["hookSpecificOutput"]["updatedInput"]["command"]
                .as_str()
                .unwrap(),
            "codeterm snip cargo test"
        );
    }

    #[test]
    fn render_claude_preserves_other_tool_input_fields() {
        let json = r#"{ "tool_name": "Bash", "tool_input": { "command": "x", "description": "tests" } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Claude, json).unwrap();
        let out = render_rewrite(&ctx, "codeterm snip x");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["hookSpecificOutput"]["updatedInput"]["description"]
                .as_str()
                .unwrap(),
            "tests"
        );
    }

    #[test]
    fn render_opencode_returns_flat_tool_args_shape() {
        let json = r#"{ "tool": "bash", "args": { "command": "x" } }"#;
        let ctx = parse_pre_tool_use(HookProvider::Opencode, json).unwrap();
        let out = render_rewrite(&ctx, "codeterm snip x");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"].as_str().unwrap(), "bash");
        assert_eq!(v["args"]["command"].as_str().unwrap(), "codeterm snip x");
    }

    #[test]
    fn render_generic_returns_just_command() {
        let json = r#"{ "command": "x" }"#;
        let ctx = parse_pre_tool_use(HookProvider::Generic, json).unwrap();
        let out = render_rewrite(&ctx, "codeterm snip x");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["command"].as_str().unwrap(), "codeterm snip x");
    }

    #[test]
    fn render_passthrough_is_empty() {
        assert!(render_passthrough().is_empty());
    }

    #[test]
    fn is_bash_recognises_capitalised_and_lower() {
        for n in ["Bash", "bash", "shell", "Shell"] {
            let ctx = PreToolUseCtx {
                provider: HookProvider::Claude,
                tool_name: n.into(),
                command: None,
                raw: Value::Null,
            };
            assert!(is_bash(&ctx), "{} should be recognised", n);
        }
    }

    #[test]
    fn is_bash_rejects_other_tools() {
        for n in ["Read", "Write", "Edit", "Glob", "Task"] {
            let ctx = PreToolUseCtx {
                provider: HookProvider::Claude,
                tool_name: n.into(),
                command: None,
                raw: Value::Null,
            };
            assert!(!is_bash(&ctx), "{} should not be recognised", n);
        }
    }
}
