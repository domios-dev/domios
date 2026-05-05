//! PreToolUse hook handler. For every Bash invocation, rewrites the
//! command to be wrapped by `codeterm snip` so test/build/log output
//! gets routed through the per-tool truncators from Phase 1.
//!
//! Wrapping rules (mirrors opencode-snip's `tool.execute.before` logic
//! but driven by the parser primitives in `cmd::snip::parser`):
//!
//! * Already-wrapped commands (start with `codeterm snip ` or `snip `)
//!   are left alone — re-wrapping would double-truncate.
//! * Shell builtins (`cd`, `source`, `export`, …) are passed through
//!   verbatim — wrapping them either has no effect or breaks the parent
//!   shell's state.
//! * Pipes: only the head of `a | b | c` is wrapped. The pipe consumers
//!   already see filtered input from the wrapped head, and wrapping
//!   them too would double-truncate the same bytes.
//! * Top-level operators (`&&`, `||`, `;`, trailing `&`): each
//!   sub-segment is wrapped independently. Builtins inside the chain
//!   stay untouched.
//! * Env-var prefixes (`KEY=val cargo test`) are preserved on the left
//!   of the wrap so the child still sees them.

use crate::args::hook::HookProvider;
use crate::cmd::hook::provider::{
    is_bash, parse_pre_tool_use, render_passthrough, render_rewrite,
};
use crate::cmd::snip::parser::{
    find_first_pipe, is_shell_builtin, split_env_prefix, split_on_operators, tokenize, Segment,
};

const SNIP_PREFIX: &str = "codeterm snip ";
const SHORT_SNIP_PREFIX: &str = "snip ";

pub fn handle(provider: HookProvider, input: &str) -> String {
    let ctx = match parse_pre_tool_use(provider, input) {
        Some(c) => c,
        None => return render_passthrough(),
    };
    if !is_bash(&ctx) {
        return render_passthrough();
    }
    let original = match ctx.command.as_deref() {
        Some(c) if !c.trim().is_empty() => c,
        _ => return render_passthrough(),
    };
    let wrapped = wrap_command(original);
    if wrapped == original {
        return render_passthrough();
    }
    render_rewrite(&ctx, &wrapped)
}

/// Pure-function rewriter — exposed for testing without the JSON
/// round-trip. Returns the (possibly modified) command string.
pub fn wrap_command(command: &str) -> String {
    if let Some(idx) = find_first_pipe(command) {
        let head = &command[..idx];
        let rest = &command[idx..];
        let wrapped_head = wrap_segment_chain(head);
        return format!("{}{}", wrapped_head, rest);
    }
    wrap_segment_chain(command)
}

/// Wrap every command segment in a top-level operator chain (`a && b ; c`).
/// Operators are echoed verbatim; commands get `wrap_one` applied.
fn wrap_segment_chain(s: &str) -> String {
    let segments = split_on_operators(s);
    if segments.len() <= 1 {
        return wrap_one(s);
    }
    segments
        .into_iter()
        .map(|seg| match seg {
            Segment::Command(c) => wrap_one(&c),
            Segment::Operator(o) => o,
        })
        .collect::<String>()
}

/// Wrap a single command segment. Preserves leading/trailing whitespace
/// and the env-var prefix; refuses to wrap shell builtins and
/// already-wrapped invocations.
fn wrap_one(seg: &str) -> String {
    if seg.trim().is_empty() {
        return seg.to_string();
    }
    let leading_ws_len = seg.len() - seg.trim_start().len();
    let trailing_ws_len = seg.len() - seg.trim_end().len();
    let leading = &seg[..leading_ws_len];
    let trailing = &seg[seg.len() - trailing_ws_len..];
    let body = &seg[leading_ws_len..seg.len() - trailing_ws_len];
    if body.is_empty() {
        return seg.to_string();
    }
    if body.starts_with(SNIP_PREFIX) || body.starts_with(SHORT_SNIP_PREFIX) {
        return seg.to_string();
    }
    let (env_prefix, rest) = split_env_prefix(body);
    let bare = rest.trim();
    if bare.is_empty() {
        return seg.to_string();
    }
    let head = match tokenize(bare).into_iter().next() {
        Some(h) => h,
        None => return seg.to_string(),
    };
    if is_shell_builtin(&head) {
        return seg.to_string();
    }
    if head.starts_with(SNIP_PREFIX) || bare.starts_with(SNIP_PREFIX) {
        return seg.to_string();
    }
    format!("{}{}{}{}", leading, env_prefix, format!("{}{}", SNIP_PREFIX, rest), trailing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_simple_cargo_test() {
        assert_eq!(wrap_command("cargo test"), "codeterm snip cargo test");
    }

    #[test]
    fn wraps_with_args() {
        assert_eq!(
            wrap_command("cargo test -p mem --lib"),
            "codeterm snip cargo test -p mem --lib"
        );
    }

    #[test]
    fn skips_already_wrapped_codeterm_snip() {
        assert_eq!(
            wrap_command("codeterm snip cargo test"),
            "codeterm snip cargo test"
        );
    }

    #[test]
    fn skips_already_wrapped_snip() {
        assert_eq!(wrap_command("snip cargo test"), "snip cargo test");
    }

    #[test]
    fn skips_shell_builtin_cd() {
        assert_eq!(wrap_command("cd /tmp"), "cd /tmp");
    }

    #[test]
    fn skips_shell_builtin_source() {
        assert_eq!(wrap_command("source ./env.sh"), "source ./env.sh");
    }

    #[test]
    fn skips_shell_builtin_export() {
        assert_eq!(wrap_command("export FOO=bar"), "export FOO=bar");
    }

    #[test]
    fn skips_shell_builtin_eval() {
        assert_eq!(wrap_command("eval $(ssh-agent -s)"), "eval $(ssh-agent -s)");
    }

    #[test]
    fn preserves_env_prefix() {
        assert_eq!(
            wrap_command("RUST_LOG=debug cargo test"),
            "RUST_LOG=debug codeterm snip cargo test"
        );
    }

    #[test]
    fn preserves_multiple_env_prefix() {
        assert_eq!(
            wrap_command("RUST_LOG=debug FOO=bar cargo test"),
            "RUST_LOG=debug FOO=bar codeterm snip cargo test"
        );
    }

    #[test]
    fn pipe_wraps_only_head() {
        assert_eq!(
            wrap_command("cargo test | grep FAIL"),
            "codeterm snip cargo test | grep FAIL"
        );
    }

    #[test]
    fn pipe_chain_wraps_only_first() {
        assert_eq!(
            wrap_command("git log -200 | head -50 | wc -l"),
            "codeterm snip git log -200 | head -50 | wc -l"
        );
    }

    #[test]
    fn logical_and_wraps_each_segment() {
        assert_eq!(
            wrap_command("cargo build && cargo test"),
            "codeterm snip cargo build && codeterm snip cargo test"
        );
    }

    #[test]
    fn logical_or_wraps_each_segment() {
        assert_eq!(
            wrap_command("cargo test || cargo clean"),
            "codeterm snip cargo test || codeterm snip cargo clean"
        );
    }

    #[test]
    fn semicolon_wraps_each_segment() {
        assert_eq!(
            wrap_command("cargo build; cargo test"),
            "codeterm snip cargo build; codeterm snip cargo test"
        );
    }

    #[test]
    fn semicolon_with_builtin_skips_builtin() {
        assert_eq!(
            wrap_command("cd /tmp; cargo test"),
            "cd /tmp; codeterm snip cargo test"
        );
    }

    #[test]
    fn double_amp_with_builtin_skips_builtin() {
        assert_eq!(
            wrap_command("cd /tmp && cargo test"),
            "cd /tmp && codeterm snip cargo test"
        );
    }

    #[test]
    fn empty_command_unchanged() {
        assert_eq!(wrap_command(""), "");
    }

    #[test]
    fn whitespace_only_command_unchanged() {
        assert_eq!(wrap_command("   "), "   ");
    }

    #[test]
    fn preserves_quoted_args() {
        assert_eq!(
            wrap_command("git log --grep \"hello world\""),
            "codeterm snip git log --grep \"hello world\""
        );
    }

    #[test]
    fn quoted_pipe_inside_args_not_split() {
        assert_eq!(
            wrap_command("echo 'a | b' && cargo test"),
            "codeterm snip echo 'a | b' && codeterm snip cargo test"
        );
    }

    #[test]
    fn handles_unknown_tool_passthrough_wrap() {
        // Unknown tool → snip will identity-passthrough, but wrap is still
        // safe (gives no-op truncation, agent gets full output).
        assert_eq!(wrap_command("ruby -e 'puts 1'"), "codeterm snip ruby -e 'puts 1'");
    }

    #[test]
    fn handle_claude_bash_returns_rewritten_json() {
        let input = r#"{
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }"#;
        let out = handle(HookProvider::Claude, input);
        assert!(out.contains("\"hookSpecificOutput\""));
        assert!(out.contains("\"permissionDecision\":\"allow\""));
        assert!(out.contains("\"command\":\"codeterm snip cargo test\""));
    }

    #[test]
    fn handle_claude_already_wrapped_returns_passthrough() {
        let input = r#"{ "tool_name": "Bash", "tool_input": { "command": "codeterm snip cargo test" } }"#;
        assert_eq!(handle(HookProvider::Claude, input), "");
    }

    #[test]
    fn handle_claude_builtin_returns_passthrough() {
        let input = r#"{ "tool_name": "Bash", "tool_input": { "command": "cd /tmp" } }"#;
        assert_eq!(handle(HookProvider::Claude, input), "");
    }

    #[test]
    fn handle_claude_non_bash_returns_passthrough() {
        let input = r#"{ "tool_name": "Read", "tool_input": { "path": "/tmp/x" } }"#;
        assert_eq!(handle(HookProvider::Claude, input), "");
    }

    #[test]
    fn handle_claude_empty_command_returns_passthrough() {
        let input = r#"{ "tool_name": "Bash", "tool_input": { "command": "" } }"#;
        assert_eq!(handle(HookProvider::Claude, input), "");
    }

    #[test]
    fn handle_claude_invalid_json_returns_passthrough() {
        assert_eq!(handle(HookProvider::Claude, "not json"), "");
    }

    #[test]
    fn handle_opencode_bash_returns_rewritten_json() {
        let input = r#"{ "tool": "bash", "args": { "command": "go test ./..." } }"#;
        let out = handle(HookProvider::Opencode, input);
        assert!(out.contains("\"command\":\"codeterm snip go test ./...\""));
        assert!(out.contains("\"tool\":\"bash\""));
    }

    #[test]
    fn handle_generic_flat_returns_command_only() {
        let input = r#"{ "command": "pytest -v" }"#;
        let out = handle(HookProvider::Generic, input);
        assert!(out.contains("\"command\":\"codeterm snip pytest -v\""));
    }

    #[test]
    fn wrap_pipe_with_env_prefix_preserves_env() {
        assert_eq!(
            wrap_command("RUST_LOG=debug cargo test | head"),
            "RUST_LOG=debug codeterm snip cargo test | head"
        );
    }

    #[test]
    fn wrap_with_trailing_amp_keeps_amp() {
        assert_eq!(
            wrap_command("cargo test &"),
            "codeterm snip cargo test &"
        );
    }

    #[test]
    fn wrap_preserves_leading_whitespace_in_segment() {
        // After a `&&`, the next segment usually has a leading space.
        assert_eq!(
            wrap_command("cargo build && cargo test"),
            "codeterm snip cargo build && codeterm snip cargo test"
        );
    }
}
