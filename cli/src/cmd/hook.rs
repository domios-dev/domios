//! `codeterm hook <event>` — provider-agnostic hook handler.
//!
//! Reads the provider's JSON from stdin, dispatches on `<event>` to the
//! matching handler, writes the (possibly modified) JSON to stdout.
//! Cold-start budget: <50 ms on a release build — keep this entry point
//! small and avoid heavy crate initialisation. The dispatcher
//! short-circuits in `main.rs` before the daemon socket is touched.

pub mod compact_injection;
pub mod events;
pub mod provider;

use std::io::{Read, Write};

use crate::args::hook::{HookAction, HookProvider};

pub fn handle_hook(action: HookAction) {
    let provider = HookProvider::from_env();

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        input.clear();
    }

    let output = match action {
        HookAction::SessionStart => events::session_start::handle(provider, &input),
        HookAction::UserPromptSubmit => events::user_prompt_submit::handle(provider, &input),
        HookAction::PreToolUse => events::pre_tool_use::handle(provider, &input),
        HookAction::PreCompact => events::pre_compact::handle(provider, &input),
    };

    if !output.is_empty() {
        let _ = std::io::stdout().write_all(output.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::hook::HookAction;

    #[test]
    fn dispatch_returns_empty_for_session_start() {
        // CODETERM_HOOK_NO_MEM forces the mem-wake-up subprocess off so the
        // test is deterministic regardless of whether `codeterm` is on PATH.
        std::env::set_var("CODETERM_HOOK_NO_MEM", "1");
        let out = events::session_start::handle(HookProvider::Claude, "{}");
        std::env::remove_var("CODETERM_HOOK_NO_MEM");
        assert_eq!(out, "");
        // Tag the variant so this test fails if the enum drifts.
        let _ = HookAction::SessionStart;
    }

    #[test]
    fn dispatch_returns_empty_for_user_prompt_submit() {
        assert_eq!(
            events::user_prompt_submit::handle(HookProvider::Claude, "{}"),
            ""
        );
        let _ = HookAction::UserPromptSubmit;
    }

    #[test]
    fn dispatch_returns_empty_for_pre_compact() {
        assert_eq!(events::pre_compact::handle(HookProvider::Claude, "{}"), "");
        let _ = HookAction::PreCompact;
    }

    #[test]
    fn dispatch_pre_tool_use_rewrites_bash_command() {
        let json = r#"{ "tool_name": "Bash", "tool_input": { "command": "cargo test" } }"#;
        let out = events::pre_tool_use::handle(HookProvider::Claude, json);
        assert!(out.contains("codeterm snip cargo test"));
        let _ = HookAction::PreToolUse;
    }
}
