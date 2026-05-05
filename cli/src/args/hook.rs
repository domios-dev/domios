use clap::Subcommand;

/// `codeterm hook <event>` — provider-agnostic hook handler. Reads
/// provider JSON from stdin, applies per-event logic, writes the
/// (possibly modified) JSON to stdout. The provider is identified via
/// `CODETERM_HOOK_SOURCE` env var (set at install time by Phase 3).
#[derive(Subcommand, Debug)]
pub enum HookAction {
    /// Fired once at session start. Injects identity (codeterm mem
    /// wake-up), the original task, surviving `<command>` directives
    /// and the workspace's AGENTS.md / CLAUDE.md / .codeterm/repo.md
    /// via `hookSpecificOutput.additionalContext` — the bits roo-code
    /// and plandex preserve across condensings, surfaced where Claude
    /// actually reads them.
    SessionStart,

    /// Fired before each user prompt is submitted. Stub in Phase 2 —
    /// Phase 5 fills it with tool-history-aware tips (e.g. "prefix this
    /// with `codeterm snip` for ~99% output reduction").
    UserPromptSubmit,

    /// Fired before a tool call executes. For Bash, wraps the command
    /// with `codeterm snip` so test/build/log output gets auto-truncated.
    PreToolUse,

    /// Fired before context compaction. Emits the same identity /
    /// original-task / `<command>` / workspace-rules block as
    /// SessionStart, on stdout (Claude Code's PreCompact protocol
    /// doesn't surface it, but harmless; SessionStart is the
    /// load-bearing path).
    PreCompact,
}

/// Identifies the provider whose JSON shape is on stdin. Defaults to
/// Claude when `CODETERM_HOOK_SOURCE` is unset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookProvider {
    Claude,
    Opencode,
    Codex,
    Generic,
}

impl HookProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "opencode" => HookProvider::Opencode,
            "codex" => HookProvider::Codex,
            "generic" => HookProvider::Generic,
            _ => HookProvider::Claude,
        }
    }

    pub fn from_env() -> Self {
        Self::from_str(&std::env::var("CODETERM_HOOK_SOURCE").unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_empty_is_claude() {
        assert_eq!(HookProvider::from_str(""), HookProvider::Claude);
    }

    #[test]
    fn from_str_opencode_lowercase() {
        assert_eq!(HookProvider::from_str("opencode"), HookProvider::Opencode);
    }

    #[test]
    fn from_str_opencode_uppercase() {
        assert_eq!(HookProvider::from_str("OPENCODE"), HookProvider::Opencode);
    }

    #[test]
    fn from_str_codex() {
        assert_eq!(HookProvider::from_str("codex"), HookProvider::Codex);
        assert_eq!(HookProvider::from_str("CODEX"), HookProvider::Codex);
    }

    #[test]
    fn from_str_generic() {
        assert_eq!(HookProvider::from_str("generic"), HookProvider::Generic);
    }

    #[test]
    fn from_str_claude_explicit() {
        assert_eq!(HookProvider::from_str("claude"), HookProvider::Claude);
    }

    #[test]
    fn from_str_unknown_falls_back_to_claude() {
        assert_eq!(HookProvider::from_str("frobnicate"), HookProvider::Claude);
    }

    #[test]
    fn from_str_with_whitespace_padding_does_not_match() {
        // We don't trim on purpose — the caller controls the env var.
        assert_eq!(HookProvider::from_str(" opencode "), HookProvider::Claude);
    }
}
