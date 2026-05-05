//! `session_start` hook handler.
//!
//! Fires once at the beginning of an agent session. We use
//! `hookSpecificOutput.additionalContext` to inject the same kind of
//! identity / workspace-rules block that PreCompact tries to preserve —
//! this is the hook event whose output Claude Code DOES surface in the
//! next turn's context, so the patterns from roo-code's
//! `extractCommandBlocks` and plandex's "first-user-message-is-sacred"
//! actually reach the model here (rather than via the PreCompact stdout
//! that Claude doesn't read).

use crate::args::hook::HookProvider;
use crate::cmd::hook::compact_injection::{build_block, InjectionSources};
use serde_json::Value;

pub fn handle(provider: HookProvider, input: &str) -> String {
    let sources = sources_from_input(input);
    let block = build_block(&sources);
    if block.is_empty() {
        return String::new();
    }
    let payload = block.render();
    render_session_start(provider, &payload)
}

fn sources_from_input(input: &str) -> InjectionSources {
    let v: Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return InjectionSources::default(),
    };
    InjectionSources {
        transcript_path: v
            .get("transcript_path")
            .and_then(Value::as_str)
            .map(str::to_string),
        cwd: v.get("cwd").and_then(Value::as_str).map(str::to_string),
        include_mem_wakeup: true,
    }
}

fn render_session_start(provider: HookProvider, additional_context: &str) -> String {
    match provider {
        HookProvider::Claude | HookProvider::Codex => serde_json::to_string(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": additional_context,
            }
        }))
        .unwrap_or_default(),
        // OpenCode and Generic don't have a documented additionalContext
        // shape yet — return the raw payload so the Phase 3 shim can
        // splice it into whatever surface the host expects.
        HookProvider::Opencode | HookProvider::Generic => additional_context.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_for_garbage_input() {
        assert_eq!(handle(HookProvider::Claude, "not json"), "");
    }

    #[test]
    fn returns_empty_when_nothing_to_inject() {
        std::env::set_var("CODETERM_HOOK_NO_MEM", "1");
        let out = handle(HookProvider::Claude, "{}");
        std::env::remove_var("CODETERM_HOOK_NO_MEM");
        assert_eq!(out, "");
    }

    #[test]
    fn injects_workspace_rules_via_additional_context() {
        std::env::set_var("CODETERM_HOOK_NO_MEM", "1");
        let dir = std::env::temp_dir().join(format!(
            "codeterm-session-start-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("AGENTS.md"), "be precise").unwrap();
        let input = format!(r#"{{"cwd":{:?}}}"#, dir.to_string_lossy());
        let out = handle(HookProvider::Claude, &input);
        std::env::remove_var("CODETERM_HOOK_NO_MEM");
        let _ = std::fs::remove_dir_all(&dir);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["hookSpecificOutput"]["hookEventName"]
                .as_str()
                .unwrap(),
            "SessionStart"
        );
        let ac = v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(ac.contains("be precise"));
        assert!(ac.contains("Workspace rules"));
    }

    #[test]
    fn opencode_returns_raw_payload() {
        std::env::set_var("CODETERM_HOOK_NO_MEM", "1");
        let dir = std::env::temp_dir().join(format!(
            "codeterm-session-start-oc-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("AGENTS.md"), "rules").unwrap();
        let input = format!(r#"{{"cwd":{:?}}}"#, dir.to_string_lossy());
        let out = handle(HookProvider::Opencode, &input);
        std::env::remove_var("CODETERM_HOOK_NO_MEM");
        let _ = std::fs::remove_dir_all(&dir);
        // OpenCode path returns the raw injection text, NOT a JSON wrapper.
        assert!(out.starts_with("<system-reminder>"));
        assert!(out.contains("rules"));
    }
}
