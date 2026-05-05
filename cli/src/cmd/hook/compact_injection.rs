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
    pub workspace_memory: Option<String>,
    pub first_user_message: Option<String>,
    pub command_blocks: Vec<String>,
    pub mem_wakeup: Option<String>,
    pub read_files: Option<String>,
}

impl InjectionBlock {
    pub fn is_empty(&self) -> bool {
        self.workspace_rules.is_none()
            && self.workspace_memory.is_none()
            && self.first_user_message.is_none()
            && self.command_blocks.is_empty()
            && self.mem_wakeup.is_none()
            && self.read_files.is_none()
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
        if let Some(mem) = &self.workspace_memory {
            sections.push(wrap_reminder("Workspace memory (.codeterm/repo.md)", mem));
        }
        if let Some(read) = &self.read_files {
            sections.push(wrap_reminder(
                "Files this session has read (most recent first)",
                read,
            ));
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
        let cwd_path = Path::new(cwd);
        block.workspace_rules = read_workspace_rules(cwd_path);
        block.workspace_memory = read_workspace_memory(cwd_path);
    }

    if let Some(path) = sources.transcript_path.as_deref() {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let entries = parse_transcript_jsonl(&text);
        block.first_user_message = extract_first_user_message(&entries);
        block.command_blocks = extract_all_command_blocks(&entries);
        let read_paths = extract_read_file_paths(&entries);
        let cwd_path = sources.cwd.as_deref().map(Path::new);
        block.read_files = build_read_files_section(&read_paths, cwd_path);
    }

    if sources.include_mem_wakeup && std::env::var("CODETERM_HOOK_NO_MEM").is_err() {
        block.mem_wakeup = run_mem_wakeup_subprocess();
    }

    block
}

pub fn read_workspace_rules(cwd: &Path) -> Option<String> {
    const MAX_BYTES: usize = 16 * 1024;
    for name in ["AGENTS.md", "CLAUDE.md"] {
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

pub fn read_workspace_memory(cwd: &Path) -> Option<String> {
    const MAX_BYTES: usize = 8 * 1024;
    let p = cwd.join(".codeterm").join("repo.md");
    let content = std::fs::read_to_string(&p).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_BYTES).collect())
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

/// Walk transcript entries and collect file paths the agent had open.
/// Counts every `Read` / `Edit` / `Write` / `MultiEdit` / `NotebookEdit`
/// tool_use — anything that implies "the agent has this file's context."
/// Deduplicated, ordered most-recent-first. Mirrors roo-code's
/// `generateFoldedFileContext` discovery step; the read+edit set is the
/// strongest single signal of what to re-anchor after a context compact.
pub fn extract_read_file_paths(entries: &[TranscriptEntry]) -> Vec<String> {
    use std::collections::HashMap;
    const FILE_TOOLS: &[&str] = &["Read", "Edit", "Write", "MultiEdit", "NotebookEdit"];
    let mut last_pos: HashMap<String, usize> = HashMap::new();
    let mut counter: usize = 0;
    for e in entries {
        if e.role != "assistant" {
            continue;
        }
        let msg = e.raw.get("message").unwrap_or(&e.raw);
        let blocks = match msg.get("content").and_then(Value::as_array) {
            Some(b) => b,
            None => continue,
        };
        for block in blocks {
            let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
            if ty != "tool_use" {
                continue;
            }
            let name = block.get("name").and_then(Value::as_str).unwrap_or("");
            if !FILE_TOOLS.iter().any(|t| *t == name) {
                continue;
            }
            // NotebookEdit uses `notebook_path`; everything else uses
            // `file_path`. Try both keys.
            let path = block
                .get("input")
                .and_then(|i| {
                    i.get("file_path")
                        .or_else(|| i.get("notebook_path"))
                })
                .and_then(Value::as_str);
            if let Some(p) = path {
                last_pos.insert(p.to_string(), counter);
                counter += 1;
            }
        }
    }
    let mut indexed: Vec<(usize, String)> =
        last_pos.into_iter().map(|(k, v)| (v, k)).collect();
    indexed.sort_by(|a, b| b.0.cmp(&a.0));
    indexed.into_iter().map(|(_, p)| p).collect()
}

/// Render the "Files this session has read" body. For each path we emit
/// a `### <path>` header; for Rust files we also dump the per-symbol
/// signature lines via `extract_rust` (same source the `codeterm map`
/// command uses). Non-Rust files get the header only — agent at least
/// remembers the file existed across compaction.
///
/// Caps: `MAX_FILES` distinct files, `MAX_BYTES` total body length. The
/// caps are deliberately generous (matching roo-code's `foldedFileContext`
/// behaviour) — better to overspend a few hundred tokens than to lose the
/// "what was I looking at" anchor.
pub fn build_read_files_section(paths: &[String], cwd: Option<&Path>) -> Option<String> {
    const MAX_FILES: usize = 30;
    const MAX_BYTES: usize = 32 * 1024;
    if paths.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut any = false;
    for raw in paths.iter().take(MAX_FILES) {
        let resolved = resolve_read_path(raw, cwd);
        if !resolved.is_file() {
            continue;
        }
        let mut block = String::new();
        block.push_str("### ");
        block.push_str(raw);
        block.push('\n');
        if let Some(lang) = crate::cmd::map::extract::Lang::from_path(&resolved) {
            if let Ok(source) = std::fs::read_to_string(&resolved) {
                let mut tags = crate::cmd::map::extract::extract(&source, lang);
                tags.sort_by_key(|t| t.line);
                for t in tags {
                    block.push_str(raw);
                    block.push(':');
                    block.push_str(&t.line.to_string());
                    block.push_str(" | ");
                    block.push_str(&t.signature);
                    block.push('\n');
                }
            }
        }
        if out.len() + block.len() > MAX_BYTES {
            break;
        }
        if any {
            out.push('\n');
        }
        out.push_str(&block);
        any = true;
    }
    if !any {
        return None;
    }
    Some(out)
}

fn resolve_read_path(raw: &str, cwd: Option<&Path>) -> std::path::PathBuf {
    let pb = std::path::PathBuf::from(raw);
    if pb.is_absolute() {
        return pb;
    }
    match cwd {
        Some(c) => c.join(pb),
        None => pb,
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
    fn workspace_memory_loaded_from_codeterm_repo_md() {
        let dir = tempdir_with(&[(".codeterm/repo.md", "Workspace memory.")]);
        assert_eq!(
            read_workspace_memory(dir.path()),
            Some("Workspace memory.".to_string())
        );
        assert_eq!(read_workspace_rules(dir.path()), None);
    }

    #[test]
    fn workspace_rules_prefers_agents_md_over_claude_md() {
        let dir = tempdir_with(&[("AGENTS.md", "first"), ("CLAUDE.md", "second")]);
        assert_eq!(read_workspace_rules(dir.path()), Some("first".to_string()));
    }

    #[test]
    fn workspace_rules_and_memory_independent() {
        let dir = tempdir_with(&[
            ("AGENTS.md", "rules text"),
            (".codeterm/repo.md", "memory text"),
        ]);
        assert_eq!(read_workspace_rules(dir.path()), Some("rules text".into()));
        assert_eq!(read_workspace_memory(dir.path()), Some("memory text".into()));
    }

    #[test]
    fn render_includes_workspace_memory_after_rules() {
        let block = InjectionBlock {
            workspace_rules: Some("rules".into()),
            workspace_memory: Some("memory".into()),
            ..Default::default()
        };
        let s = block.render();
        let r_pos = s.find("Workspace rules").unwrap();
        let m_pos = s.find("Workspace memory").unwrap();
        assert!(r_pos < m_pos);
        assert!(s.contains("memory"));
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
            workspace_memory: None,
            first_user_message: Some("task".into()),
            command_blocks: vec!["<command>x</command>".into()],
            mem_wakeup: Some("identity".into()),
            read_files: None,
        };
        let s = block.render();
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

    fn read_call_raw(file_path: &str) -> Value {
        serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "name": "Read",
                    "input": {"file_path": file_path}
                }]
            }
        })
    }

    fn assistant_entry(raw: Value) -> TranscriptEntry {
        TranscriptEntry {
            role: "assistant".to_string(),
            text: String::new(),
            raw,
        }
    }

    fn edit_call_raw(file_path: &str) -> Value {
        serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "name": "Edit",
                    "input": {"file_path": file_path}
                }]
            }
        })
    }

    fn write_call_raw(file_path: &str) -> Value {
        serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "name": "Write",
                    "input": {"file_path": file_path}
                }]
            }
        })
    }

    fn notebook_edit_call_raw(notebook_path: &str) -> Value {
        serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "name": "NotebookEdit",
                    "input": {"notebook_path": notebook_path}
                }]
            }
        })
    }

    #[test]
    fn extract_read_paths_includes_edit_write_multiedit() {
        let entries = vec![
            assistant_entry(read_call_raw("/r.rs")),
            assistant_entry(edit_call_raw("/e.rs")),
            assistant_entry(write_call_raw("/w.rs")),
        ];
        let paths = extract_read_file_paths(&entries);
        assert!(paths.contains(&"/r.rs".to_string()));
        assert!(paths.contains(&"/e.rs".to_string()));
        assert!(paths.contains(&"/w.rs".to_string()));
    }

    #[test]
    fn extract_read_paths_includes_notebook_edit_via_notebook_path() {
        let entries = vec![assistant_entry(notebook_edit_call_raw("/n.ipynb"))];
        let paths = extract_read_file_paths(&entries);
        assert_eq!(paths, vec!["/n.ipynb"]);
    }

    #[test]
    fn extract_read_paths_two_reads_same_entry_keeps_last() {
        // Two Read tool_uses inside one assistant message: counter
        // advances per block, so the second occurrence sets the
        // most-recent position. Lock the contract.
        let entries = vec![assistant_entry(serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [
                    {"type":"tool_use","name":"Read","input":{"file_path":"/a.rs"}},
                    {"type":"tool_use","name":"Read","input":{"file_path":"/b.rs"}},
                    {"type":"tool_use","name":"Read","input":{"file_path":"/a.rs"}}
                ]
            }
        }))];
        let paths = extract_read_file_paths(&entries);
        // /a.rs appears twice but dedupes; its last-seen position is
        // after /b.rs, so /a.rs ranks first.
        assert_eq!(paths, vec!["/a.rs", "/b.rs"]);
    }

    #[test]
    fn extract_read_paths_dedupes_and_orders_recent_first() {
        let entries = vec![
            assistant_entry(read_call_raw("/a.rs")),
            assistant_entry(read_call_raw("/b.rs")),
            assistant_entry(read_call_raw("/a.rs")),
            assistant_entry(read_call_raw("/c.rs")),
        ];
        let paths = extract_read_file_paths(&entries);
        assert_eq!(paths, vec!["/c.rs", "/a.rs", "/b.rs"]);
    }

    #[test]
    fn extract_read_paths_ignores_non_read_tools() {
        let entries = vec![assistant_entry(serde_json::json!({
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
                    {"type": "tool_use", "name": "Read", "input": {"file_path": "/x.rs"}}
                ]
            }
        }))];
        assert_eq!(extract_read_file_paths(&entries), vec!["/x.rs"]);
    }

    #[test]
    fn extract_read_paths_ignores_user_role() {
        let entries = vec![TranscriptEntry {
            role: "user".to_string(),
            text: String::new(),
            raw: read_call_raw("/x.rs"),
        }];
        assert!(extract_read_file_paths(&entries).is_empty());
    }

    #[test]
    fn build_read_files_section_emits_signatures_for_rust() {
        let dir = tempdir_with(&[("alpha.rs", "pub fn alpha() {}\npub struct Beta;\n")]);
        let abs = dir.path().join("alpha.rs").to_string_lossy().into_owned();
        let body = build_read_files_section(&[abs.clone()], None).unwrap();
        assert!(body.contains("### "));
        assert!(body.contains(&abs));
        assert!(body.contains("alpha"));
        assert!(body.contains("Beta"));
    }

    #[test]
    fn build_read_files_section_resolves_relative_against_cwd() {
        let dir = tempdir_with(&[("foo.rs", "pub fn foo() {}\n")]);
        let body = build_read_files_section(&["foo.rs".into()], Some(dir.path())).unwrap();
        assert!(body.contains("### foo.rs"));
        assert!(body.contains("foo"));
    }

    #[test]
    fn build_read_files_section_skips_missing_files() {
        let body = build_read_files_section(&["/nonexistent/zzz.rs".into()], None);
        assert!(body.is_none());
    }

    #[test]
    fn build_read_files_section_emits_header_only_for_non_rust() {
        let dir = tempdir_with(&[("README.md", "# heading\n")]);
        let abs = dir.path().join("README.md").to_string_lossy().into_owned();
        let body = build_read_files_section(&[abs.clone()], None).unwrap();
        assert!(body.contains("### "));
        assert!(body.contains(&abs));
        assert!(!body.contains("|"));
    }

    #[test]
    fn build_read_files_section_caps_total_files() {
        let dir = tempdir_with(&[]);
        let mut paths = Vec::new();
        for i in 0..50 {
            let name = format!("f{}.rs", i);
            let p = dir.path().join(&name);
            std::fs::write(&p, format!("pub fn f{}() {{}}\n", i)).unwrap();
            paths.push(p.to_string_lossy().into_owned());
        }
        let body = build_read_files_section(&paths, None).unwrap();
        let header_count = body.matches("### ").count();
        assert!(header_count <= 30, "got {} headers", header_count);
        assert!(header_count > 0);
    }

    #[test]
    fn build_block_includes_read_files_section() {
        let dir = tempdir_with(&[("zeta.rs", "pub fn zeta_fn() {}\n")]);
        let transcript = dir.path().join("t.jsonl");
        let abs = dir.path().join("zeta.rs").to_string_lossy().into_owned();
        let body = format!(
            r#"{{"role":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"Read","input":{{"file_path":"{}"}}}}]}}}}"#,
            abs.replace('\\', "\\\\")
        );
        std::fs::write(&transcript, body).unwrap();
        let sources = InjectionSources {
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..Default::default()
        };
        let block = build_block(&sources);
        assert!(block.read_files.is_some());
        let rendered = block.render();
        assert!(rendered.contains("Files this session has read"));
        assert!(rendered.contains("zeta_fn"));
    }

    #[test]
    fn render_places_read_files_after_workspace_memory() {
        let block = InjectionBlock {
            workspace_memory: Some("memory".into()),
            read_files: Some("### foo.rs\nfoo.rs:1 | fn foo()".into()),
            ..Default::default()
        };
        let s = block.render();
        let mem_pos = s.find("Workspace memory").unwrap();
        let read_pos = s.find("Files this session has read").unwrap();
        assert!(mem_pos < read_pos);
        assert!(s.contains("foo.rs"));
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
