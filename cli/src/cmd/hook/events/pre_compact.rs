//! `pre_compact` hook handler.
//!
//! Per the v1.2.0 plan we emit the assembled identity / first-user-task /
//! `<command>` / workspace-rules block on stdout so that — IF the
//! provider's PreCompact protocol surfaces stdout into the post-compact
//! context — condense doesn't drop those things.
//!
//! Reality check: Claude Code's PreCompact JSON protocol only honours
//! `decision: "block"` on stdout; `additionalContext` and free-form text
//! are NOT picked up here. So for the Claude case the assembled block
//! is still emitted (it's harmless — Claude just ignores it), but the
//! same content is also injected from `session_start` and (in Phase 5)
//! `user_prompt_submit`, which DO support `additionalContext`. That's
//! how we actually get the patterns from roo-code/plandex back into
//! the model's context.
//!
//! No-op cases (return empty stdout): non-Claude providers we haven't
//! mapped yet, transcripts we can't read, and `CODETERM_HOOK_NO_MEM=1`
//! tests.

use crate::args::hook::HookProvider;
use crate::cmd::hook::compact_injection::{build_block, InjectionSources};
use serde_json::Value;

pub fn handle(_provider: HookProvider, input: &str) -> String {
    let sources = sources_from_input(input);
    let block = build_block(&sources);
    if block.is_empty() {
        return String::new();
    }
    block.render()
}

/// Extract `transcript_path` and `cwd` from the PreCompact stdin JSON.
/// Both fields are present in the documented Claude PreCompact payload;
/// other providers may omit them, which just means we skip those
/// extraction sources.
pub(crate) fn sources_from_input(input: &str) -> InjectionSources {
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
        // Subprocess to `codeterm mem wake-up` is the most expensive
        // source — leave it OFF in PreCompact (best-effort; the
        // SessionStart path runs it instead, where stdout actually
        // reaches the model).
        include_mem_wakeup: false,
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
    fn returns_empty_for_empty_input() {
        assert_eq!(handle(HookProvider::Claude, ""), "");
    }

    #[test]
    fn returns_empty_when_no_extraction_sources_apply() {
        // No transcript_path, no cwd — nothing to extract.
        assert_eq!(handle(HookProvider::Claude, "{}"), "");
    }

    #[test]
    fn emits_workspace_rules_when_cwd_has_agents_md() {
        let dir = std::env::temp_dir().join(format!(
            "codeterm-pre-compact-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("AGENTS.md"), "# rules\nbe careful").unwrap();
        let json = format!(
            r#"{{"hook_event_name":"PreCompact","cwd":{:?}}}"#,
            dir.to_string_lossy()
        );
        let out = handle(HookProvider::Claude, &json);
        assert!(out.contains("Workspace rules"));
        assert!(out.contains("be careful"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emits_command_blocks_from_transcript() {
        let dir = std::env::temp_dir().join(format!(
            "codeterm-pre-compact-tt-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let transcript = dir.join("t.jsonl");
        std::fs::write(
            &transcript,
            r#"{"role":"user","content":"work <command>persist this</command>"}"#,
        )
        .unwrap();
        let json = format!(
            r#"{{"transcript_path":{:?}}}"#,
            transcript.to_string_lossy()
        );
        let out = handle(HookProvider::Claude, &json);
        assert!(out.contains("<command>persist this</command>"));
        assert!(out.contains("Active <command>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sources_from_input_parses_known_fields() {
        let json = r#"{"transcript_path":"/tmp/x.jsonl","cwd":"/foo","trigger":"auto"}"#;
        let s = sources_from_input(json);
        assert_eq!(s.transcript_path.as_deref(), Some("/tmp/x.jsonl"));
        assert_eq!(s.cwd.as_deref(), Some("/foo"));
        assert!(!s.include_mem_wakeup);
    }
}
