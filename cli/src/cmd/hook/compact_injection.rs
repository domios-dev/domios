//! Compact-injection content builder. Used by `pre_compact` (per the
//! v1.2.0 plan, written to stdout — Claude's PreCompact protocol may or
//! may not preserve it) and by `session_start` (where Claude's
//! `hookSpecificOutput.additionalContext` reliably surfaces it on the
//! next turn).
//!
//! Patterns mimicked from the references the plan calls out:
//!
//! * **roo-code** (`references/roo-code/src/core/condense/index.ts`,
//!   `extractCommandBlocks`) — `<command…>…</command>` blocks from the
//!   original task survive every condense as "active workflows."
//!   Roo wraps them in `<system-reminder>## Active Workflows…</system-reminder>`.
//! * **plandex** (`references/plandex/app/server/model/plan/tell_summary.go`)
//!   — the "first user message is sacred" rule: the original task
//!   description is preserved verbatim, separate from any summary.
//! * Workspace `AGENTS.md` / `CLAUDE.md` is the codeterm equivalent of
//!   roo-code's persistent system prompt — load it from the hook's `cwd`
//!   and re-inject so condense doesn't lose project-level rules.
//!
//! mem-wake-up integration (the L0+L1 identity refresh) is intentionally
//! gated behind `CODETERM_HOOK_NO_MEM` so unit tests don't need a
//! running daemon. The Phase 3 installer can flip the flag on / off
//! per-machine.

use serde_json::Value;
use std::path::Path;

/// Inputs for assembling the injection block. All fields optional —
/// sources that don't apply (no transcript path, no AGENTS.md, no mem)
/// are quietly skipped.
#[derive(Debug, Default)]
pub struct InjectionSources {
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    /// Set true to call `codeterm mem wake-up` as a subprocess. Off by
    /// default (and forced off when `CODETERM_HOOK_NO_MEM=1`) to keep
    /// cold-start under the 50 ms hook latency budget.
    pub include_mem_wakeup: bool,
}

/// Assembled injection content. Empty when there's nothing to inject.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InjectionBlock {
    pub workspace_rules: Option<String>,
    pub first_user_message: Option<String>,
    pub command_blocks: Vec<String>,
    pub mem_wakeup: Option<String>,
}

impl InjectionBlock {
    pub fn is_empty(&self) -> bool {
        self.workspace_rules.is_none()
            && self.first_user_message.is_none()
            && self.command_blocks.is_empty()
            && self.mem_wakeup.is_none()
    }

    /// Render the block as a single text payload suitable for both
    /// stdout (PreCompact) and `additionalContext` (SessionStart).
    /// Each section gets its own `<system-reminder>` envelope, matching
    /// the structure roo-code emits for active workflows + folded files.
    pub fn render(&self) -> String {
        let mut sections: Vec<String> = Vec::new();
        if let Some(mem) = &self.mem_wakeup {
            sections.push(wrap_reminder("Identity (codeterm mem wake-up)", mem));
        }
        if let Some(first) = &self.first_user_message {
            sections.push(wrap_reminder(
                "Original task (sacred — never lose this)",
                first,
            ));
        }
        if !self.command_blocks.is_empty() {
            let body = self.command_blocks.join("\n\n");
            sections.push(wrap_reminder(
                "Active <command> directives (must be maintained across all condensings)",
                &body,
            ));
        }
        if let Some(rules) = &self.workspace_rules {
            sections.push(wrap_reminder("Workspace rules (AGENTS.md / CLAUDE.md)", rules));
        }
        sections.join("\n\n")
    }
}

fn wrap_reminder(heading: &str, body: &str) -> String {
    format!("<system-reminder>\n## {}\n{}\n</system-reminder>", heading, body)
}

/// Build the full injection block from the provided sources. Pure —
/// callers can substitute mocks for any field.
pub fn build_block(sources: &InjectionSources) -> InjectionBlock {
    let mut block = InjectionBlock::default();

    if let Some(cwd) = sources.cwd.as_deref() {
        block.workspace_rules = read_workspace_rules(Path::new(cwd));
    }

    if let Some(path) = sources.transcript_path.as_deref() {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let entries = parse_transcript_jsonl(&text);
        block.first_user_message = extract_first_user_message(&entries);
        block.command_blocks = extract_all_command_blocks(&entries);
    }

    if sources.include_mem_wakeup && std::env::var("CODETERM_HOOK_NO_MEM").is_err() {
        block.mem_wakeup = run_mem_wakeup_subprocess();
    }

    block
}

/// Look for `AGENTS.md`, then `CLAUDE.md`, then `.codeterm/repo.md`. The
/// first one that exists wins. Capped at ~16 KB to keep the injected
/// payload manageable.
pub fn read_workspace_rules(cwd: &Path) -> Option<String> {
    const MAX_BYTES: usize = 16 * 1024;
    for name in ["AGENTS.md", "CLAUDE.md", ".codeterm/repo.md"] {
        let p = cwd.join(name);
        if let Ok(content) = std::fs::read_to_string(&p) {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }
            let body: String = trimmed.chars().take(MAX_BYTES).collect();
            return Some(body);
        }
    }
    None
}

/// One entry from a Claude Code transcript JSONL. We only care about
/// what the agent message text looks like — the rest of the schema is
/// captured as raw `Value` in case the caller needs it.
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub role: String,
    pub text: String,
    pub raw: Value,
}

/// Parse a Claude Code transcript JSONL string into entries. Tolerant
/// of: blank lines, malformed lines (skipped), missing fields. Each
/// line is expected to be a JSON object with at least `{role, content}`
/// or `{message: {role, content}}` (Claude wraps messages in `message`).
pub fn parse_transcript_jsonl(jsonl: &str) -> Vec<TranscriptEntry> {
    let mut out: Vec<TranscriptEntry> = Vec::new();
    for line in jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let msg = v.get("message").unwrap_or(&v);
        let role = msg
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if role.is_empty() {
            continue;
        }
        let text = extract_text_from_content(msg.get("content"));
        out.push(TranscriptEntry {
            role,
            text,
            raw: v,
        });
    }
    out
}

/// Given a `content` field that's either a string or an array of
/// content blocks (Anthropic SDK shape), concatenate all the `text`
/// payloads. Tool-use blocks are rendered as `[Tool: <name>]` markers
/// so we can still spot what was happening (matches roo-code's
/// `toolUseToText` / `toolResultToText` simplification).
pub fn extract_text_from_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for block in arr {
            let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
            match ty {
                "text" => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t);
                    }
                }
                "tool_use" => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("?");
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!("[Tool: {}]", name));
                }
                "tool_result" => {
                    let is_error = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let suffix = if is_error { " (Error)" } else { "" };
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!("[Tool Result{}]", suffix));
                }
                _ => {}
            }
        }
        return out;
    }
    String::new()
}

/// First message from `role: "user"` in the transcript. Plandex calls
/// this "sacred" — it's the original task description and should never
/// be lost across condensings.
pub fn extract_first_user_message(entries: &[TranscriptEntry]) -> Option<String> {
    for e in entries {
        if e.role == "user" && !e.text.trim().is_empty() {
            const MAX: usize = 4 * 1024;
            let body: String = e.text.chars().take(MAX).collect();
            return Some(body);
        }
    }
    None
}

/// Extract every `<command…>…</command>` block from every message in the
/// transcript. Mirrors roo-code's `extractCommandBlocks` regex —
/// `<command[^>]*>[\s\S]*?<\/command>` — but in pure Rust without a
/// regex runtime so the cold-start budget stays tight.
pub fn extract_all_command_blocks(entries: &[TranscriptEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in entries {
        find_command_blocks(&e.text, &mut out);
    }
    out
}

/// Hand-rolled scanner for `<command[^>]*>(…)</command>`. Simple enough
/// that we don't need the `regex` crate here; the tag set is fixed and
/// we don't care about case-insensitivity (matches roo-code's regex).
fn find_command_blocks(haystack: &str, out: &mut Vec<String>) {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i + "<command".len() <= bytes.len() {
        if &bytes[i..i + 8] == b"<command" {
            let after = i + 8;
            // Scan to the closing '>' of the opening tag.
            let mut j = after;
            while j < bytes.len() && bytes[j] != b'>' {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            // Now search for the closing </command>.
            let close = b"</command>";
            let mut k = j + 1;
            while k + close.len() <= bytes.len() {
                if &bytes[k..k + close.len()] == close {
                    let block = &haystack[i..k + close.len()];
                    out.push(block.to_string());
                    i = k + close.len();
                    break;
                }
                k += 1;
            }
            if k + close.len() > bytes.len() {
                // Unterminated <command> — skip past the opening tag and
                // keep scanning so a later well-formed block still gets
                // picked up.
                i = j + 1;
                continue;
            }
        } else {
            i += 1;
        }
    }
}

/// Fire `codeterm mem wake-up` and return its stdout. Returns `None` if
/// the binary isn't on PATH or it errors. Caller pre-gates with the
/// `include_mem_wakeup` flag so unit tests can avoid the subprocess.
fn run_mem_wakeup_subprocess() -> Option<String> {
    use std::process::Command;
    let output = Command::new("codeterm").args(["mem", "wake-up"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_rules_finds_agents_md() {
        let dir = tempdir_with(&[("AGENTS.md", "# Project rules\nNo emojis.")]);
        assert_eq!(
            read_workspace_rules(dir.path()),
            Some("# Project rules\nNo emojis.".to_string())
        );
    }

    #[test]
    fn workspace_rules_falls_back_to_claude_md() {
        let dir = tempdir_with(&[("CLAUDE.md", "Use English in code.")]);
        assert_eq!(
            read_workspace_rules(dir.path()),
            Some("Use English in code.".to_string())
        );
    }

    #[test]
    fn workspace_rules_falls_back_to_repo_md() {
        let dir = tempdir_with(&[(".codeterm/repo.md", "Workspace memory.")]);
        assert_eq!(
            read_workspace_rules(dir.path()),
            Some("Workspace memory.".to_string())
        );
    }

    #[test]
    fn workspace_rules_prefers_agents_md_over_others() {
        let dir = tempdir_with(&[
            ("AGENTS.md", "first"),
            ("CLAUDE.md", "second"),
            (".codeterm/repo.md", "third"),
        ]);
        assert_eq!(read_workspace_rules(dir.path()), Some("first".to_string()));
    }

    #[test]
    fn workspace_rules_empty_file_is_skipped() {
        let dir = tempdir_with(&[("AGENTS.md", "   \n  \n"), ("CLAUDE.md", "fallback")]);
        assert_eq!(
            read_workspace_rules(dir.path()),
            Some("fallback".to_string())
        );
    }

    #[test]
    fn workspace_rules_none_when_nothing_present() {
        let dir = tempdir_with(&[]);
        assert_eq!(read_workspace_rules(dir.path()), None);
    }

    #[test]
    fn parse_transcript_skips_blank_lines() {
        let jsonl = "\n\n{\"role\":\"user\",\"content\":\"hi\"}\n\n";
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, "user");
        assert_eq!(entries[0].text, "hi");
    }

    #[test]
    fn parse_transcript_skips_garbage_lines() {
        let jsonl = "not json\n{\"role\":\"user\",\"content\":\"hi\"}\nstill garbage";
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "hi");
    }

    #[test]
    fn parse_transcript_handles_message_wrapper() {
        let jsonl = r#"{"message":{"role":"assistant","content":"hello"}}"#;
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, "assistant");
        assert_eq!(entries[0].text, "hello");
    }

    #[test]
    fn parse_transcript_array_content_concatenates_text_blocks() {
        let jsonl = r#"{"role":"user","content":[{"type":"text","text":"part1"},{"type":"text","text":"part2"}]}"#;
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "part1\npart2");
    }

    #[test]
    fn parse_transcript_array_content_marks_tool_use() {
        let jsonl = r#"{"role":"assistant","content":[{"type":"text","text":"calling tool"},{"type":"tool_use","name":"Bash","input":{}}]}"#;
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries[0].text, "calling tool\n[Tool: Bash]");
    }

    #[test]
    fn parse_transcript_array_content_marks_tool_result_error() {
        let jsonl = r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","is_error":true,"content":"oops"}]}"#;
        let entries = parse_transcript_jsonl(jsonl);
        assert_eq!(entries[0].text, "[Tool Result (Error)]");
    }

    #[test]
    fn parse_transcript_skips_empty_role() {
        let jsonl = r#"{"content":"orphan"}"#;
        assert!(parse_transcript_jsonl(jsonl).is_empty());
    }

    #[test]
    fn first_user_message_returns_first_user() {
        let entries = vec![
            te("assistant", "hi"),
            te("user", "task one"),
            te("user", "task two"),
        ];
        assert_eq!(
            extract_first_user_message(&entries),
            Some("task one".to_string())
        );
    }

    #[test]
    fn first_user_message_skips_empty_user_messages() {
        let entries = vec![te("user", "   "), te("user", "real task")];
        assert_eq!(
            extract_first_user_message(&entries),
            Some("real task".to_string())
        );
    }

    #[test]
    fn first_user_message_none_when_no_user() {
        let entries = vec![te("assistant", "hi")];
        assert_eq!(extract_first_user_message(&entries), None);
    }

    #[test]
    fn first_user_message_capped_at_4kb() {
        let big = "a".repeat(8192);
        let entries = vec![te("user", &big)];
        let got = extract_first_user_message(&entries).unwrap();
        assert_eq!(got.len(), 4 * 1024);
    }

    #[test]
    fn command_blocks_extracts_simple() {
        let entries = vec![te("user", "before <command>do thing</command> after")];
        let blocks = extract_all_command_blocks(&entries);
        assert_eq!(blocks, vec!["<command>do thing</command>"]);
    }

    #[test]
    fn command_blocks_extracts_multiple() {
        let entries = vec![te(
            "user",
            "<command id=1>a</command> middle <command>b</command>",
        )];
        let blocks = extract_all_command_blocks(&entries);
        assert_eq!(
            blocks,
            vec!["<command id=1>a</command>", "<command>b</command>"]
        );
    }

    #[test]
    fn command_blocks_handles_multiline_body() {
        let entries = vec![te(
            "user",
            "<command>\n  line 1\n  line 2\n</command>",
        )];
        let blocks = extract_all_command_blocks(&entries);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("line 1"));
        assert!(blocks[0].contains("line 2"));
    }

    #[test]
    fn command_blocks_skips_unterminated() {
        let entries = vec![te("user", "<command>broken")];
        assert!(extract_all_command_blocks(&entries).is_empty());
    }

    #[test]
    fn command_blocks_outer_wins_when_nested() {
        // Same behaviour as roo-code's `/<command[^>]*>[\s\S]*?<\/command>/g`
        // — the first opening tag pairs with the first closing tag,
        // which here means the outer block swallows the inner one.
        let entries = vec![te("user", "<command>broken <command>good</command>")];
        let blocks = extract_all_command_blocks(&entries);
        assert_eq!(blocks, vec!["<command>broken <command>good</command>"]);
    }

    #[test]
    fn command_blocks_empty_when_no_blocks() {
        let entries = vec![te("user", "no commands here")];
        assert!(extract_all_command_blocks(&entries).is_empty());
    }

    #[test]
    fn command_blocks_across_messages() {
        let entries = vec![
            te("user", "<command>one</command>"),
            te("assistant", "ack"),
            te("user", "<command>two</command>"),
        ];
        let blocks = extract_all_command_blocks(&entries);
        assert_eq!(
            blocks,
            vec!["<command>one</command>", "<command>two</command>"]
        );
    }

    #[test]
    fn render_block_empty_returns_empty_string() {
        let block = InjectionBlock::default();
        assert!(block.is_empty());
        assert_eq!(block.render(), "");
    }

    #[test]
    fn render_block_with_first_message() {
        let block = InjectionBlock {
            first_user_message: Some("solve x".into()),
            ..Default::default()
        };
        let s = block.render();
        assert!(s.contains("Original task"));
        assert!(s.contains("solve x"));
        assert!(s.contains("<system-reminder>"));
    }

    #[test]
    fn render_block_with_commands() {
        let block = InjectionBlock {
            command_blocks: vec!["<command>a</command>".into(), "<command>b</command>".into()],
            ..Default::default()
        };
        let s = block.render();
        assert!(s.contains("Active <command> directives"));
        assert!(s.contains("<command>a</command>"));
        assert!(s.contains("<command>b</command>"));
    }

    #[test]
    fn render_block_with_workspace_rules() {
        let block = InjectionBlock {
            workspace_rules: Some("# Project\nNo unsafe.".into()),
            ..Default::default()
        };
        let s = block.render();
        assert!(s.contains("Workspace rules"));
        assert!(s.contains("No unsafe."));
    }

    #[test]
    fn render_block_with_mem_wakeup() {
        let block = InjectionBlock {
            mem_wakeup: Some("L0 facts".into()),
            ..Default::default()
        };
        let s = block.render();
        assert!(s.contains("Identity"));
        assert!(s.contains("L0 facts"));
    }

    #[test]
    fn render_block_combines_all_sections() {
        let block = InjectionBlock {
            workspace_rules: Some("rules".into()),
            first_user_message: Some("task".into()),
            command_blocks: vec!["<command>x</command>".into()],
            mem_wakeup: Some("identity".into()),
        };
        let s = block.render();
        // Expected order: identity → first message → commands → rules.
        let id_pos = s.find("Identity").unwrap();
        let first_pos = s.find("Original task").unwrap();
        let cmd_pos = s.find("Active <command>").unwrap();
        let rules_pos = s.find("Workspace rules").unwrap();
        assert!(id_pos < first_pos);
        assert!(first_pos < cmd_pos);
        assert!(cmd_pos < rules_pos);
    }

    #[test]
    fn build_block_no_sources_is_empty() {
        let sources = InjectionSources::default();
        let block = build_block(&sources);
        assert!(block.is_empty());
    }

    #[test]
    fn build_block_reads_workspace_rules_from_cwd() {
        let dir = tempdir_with(&[("AGENTS.md", "rules text")]);
        let sources = InjectionSources {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..Default::default()
        };
        let block = build_block(&sources);
        assert_eq!(block.workspace_rules.as_deref(), Some("rules text"));
    }

    #[test]
    fn build_block_extracts_from_transcript() {
        let dir = tempdir_with(&[]);
        let transcript = dir.path().join("t.jsonl");
        let body = format!(
            "{}\n{}\n",
            r#"{"role":"user","content":"task <command>a</command>"}"#,
            r#"{"role":"assistant","content":"ack"}"#
        );
        std::fs::write(&transcript, body).unwrap();
        let sources = InjectionSources {
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let block = build_block(&sources);
        assert!(block.first_user_message.is_some());
        assert_eq!(block.command_blocks, vec!["<command>a</command>"]);
    }

    #[test]
    fn build_block_skips_mem_when_env_set() {
        std::env::set_var("CODETERM_HOOK_NO_MEM", "1");
        let sources = InjectionSources {
            include_mem_wakeup: true,
            ..Default::default()
        };
        let block = build_block(&sources);
        assert!(block.mem_wakeup.is_none());
        std::env::remove_var("CODETERM_HOOK_NO_MEM");
    }

    fn te(role: &str, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            role: role.to_string(),
            text: text.to_string(),
            raw: Value::Null,
        }
    }

    /// Mini tempdir helper — avoids pulling in the `tempfile` crate
    /// since the rest of the project doesn't have it. Cleans up via
    /// `Drop`.
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn tempdir_with(files: &[(&str, &str)]) -> TempDir {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("codeterm-hook-test-{}-{}", pid, n));
        let _ = std::fs::create_dir_all(&path);
        for (name, body) in files {
            let p = path.join(name);
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&p, body).unwrap();
        }
        TempDir { path }
    }
}
