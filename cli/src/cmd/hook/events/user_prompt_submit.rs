use std::path::{Path, PathBuf};

use crate::args::hook::HookProvider;
use crate::cmd::hook::compact_injection::{parse_transcript_jsonl, TranscriptEntry};
use crate::cmd::map::dump_single_file;
use serde_json::Value;

const TIP_SNIP: &str =
    "tip: prefix noisy build/test/log commands with `codeterm snip` for ~95% output reduction (cargo, go test, git log/diff, npm/pnpm/yarn, jest/vitest, pytest)";
const TIP_MAP_HINT: &str =
    "tip: when scanning multiple files for a symbol, `codeterm mem search \"<term>\"` against indexed skills/facts is faster than 3+ `Read` calls in a row";

pub fn handle(provider: HookProvider, input: &str) -> String {
    let v: Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let transcript_path = v.get("transcript_path").and_then(Value::as_str);
    let entries = match transcript_path {
        Some(p) => {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            parse_transcript_jsonl(&text)
        }
        None => Vec::new(),
    };
    let cwd = v.get("cwd").and_then(Value::as_str);
    let prompt = v.get("prompt").and_then(Value::as_str).unwrap_or("");

    let mut sections: Vec<String> = Vec::new();
    if let Some(loaded) = load_backticked_signatures(prompt, cwd) {
        sections.push(loaded);
    }
    if let Some(tip) = pick_tip(&entries) {
        sections.push(format!("<system-reminder>\n{}\n</system-reminder>", tip));
    }
    if sections.is_empty() {
        return String::new();
    }
    let body = sections.join("\n\n");
    render_envelope(provider, &body)
}

pub fn load_backticked_signatures(prompt: &str, cwd: Option<&str>) -> Option<String> {
    let paths = backticked_paths(prompt);
    if paths.is_empty() {
        return None;
    }
    let cwd_pb = cwd.map(PathBuf::from);
    let mut out = String::new();
    let mut any = false;
    for raw in paths.iter().take(8) {
        let resolved = resolve_path(raw, cwd_pb.as_deref());
        if !resolved.is_file() {
            continue;
        }
        if !is_supported_source(&resolved) {
            continue;
        }
        match dump_single_file(&resolved) {
            Ok(text) if !text.trim().is_empty() => {
                if any {
                    out.push('\n');
                }
                out.push_str(&text);
                any = true;
            }
            _ => {}
        }
    }
    if !any {
        return None;
    }
    Some(format!(
        "<system-reminder>\n## Signatures for paths mentioned in this prompt\n{}</system-reminder>",
        out
    ))
}

fn backticked_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'`' && bytes[j] != b'\n' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'`' {
                let token = &text[i + 1..j];
                if looks_like_path(token) {
                    out.push(token.to_string());
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn looks_like_path(s: &str) -> bool {
    if s.is_empty() || s.len() > 256 {
        return false;
    }
    if s.chars().any(|c| matches!(c, ' ' | '\t' | '"' | '\'' | '<' | '>' | '|' | '*' | '?')) {
        return false;
    }
    s.contains('/') || s.contains('.')
}

fn resolve_path(raw: &str, cwd: Option<&Path>) -> PathBuf {
    let pb = PathBuf::from(raw);
    if pb.is_absolute() {
        return pb;
    }
    match cwd {
        Some(c) => c.join(pb),
        None => pb,
    }
}

fn is_supported_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("rs")
    )
}

pub fn pick_tip(entries: &[TranscriptEntry]) -> Option<&'static str> {
    let recent = recent_assistant_bash_calls(entries, 20);
    if has_unwrapped_noisy_call(&recent) {
        return Some(TIP_SNIP);
    }
    if has_excess_reads(entries, 5, 3) {
        return Some(TIP_MAP_HINT);
    }
    None
}

fn recent_assistant_bash_calls(entries: &[TranscriptEntry], take: usize) -> Vec<String> {
    let mut out = Vec::new();
    for e in entries.iter().rev().take(take) {
        if e.role != "assistant" {
            continue;
        }
        for cmd in extract_bash_commands(&e.raw) {
            out.push(cmd);
        }
    }
    out
}

fn extract_bash_commands(raw: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let msg = raw.get("message").unwrap_or(raw);
    let content = match msg.get("content") {
        Some(c) => c,
        None => return out,
    };
    let blocks = match content.as_array() {
        Some(b) => b,
        None => return out,
    };
    for block in blocks {
        let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
        if ty != "tool_use" {
            continue;
        }
        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
        if name != "Bash" && name != "bash" {
            continue;
        }
        if let Some(cmd) = block
            .get("input")
            .and_then(|i| i.get("command"))
            .and_then(Value::as_str)
        {
            out.push(cmd.to_string());
        }
    }
    out
}

fn has_unwrapped_noisy_call(commands: &[String]) -> bool {
    let noisy = [
        "cargo test",
        "cargo build",
        "cargo check",
        "cargo clippy",
        "go test",
        "go build",
        "git log",
        "git diff",
        "npm test",
        "pnpm test",
        "yarn test",
        "jest",
        "vitest",
        "pytest",
    ];
    for cmd in commands {
        let trimmed = cmd.trim_start();
        if trimmed.starts_with("codeterm snip ") || trimmed.starts_with("snip ") {
            continue;
        }
        let bare = strip_env_prefix(trimmed);
        if noisy.iter().any(|n| bare.starts_with(n)) {
            return true;
        }
    }
    false
}

fn has_excess_reads(entries: &[TranscriptEntry], turns: usize, threshold: usize) -> bool {
    let mut reads = 0usize;
    let mut user_turns = 0usize;
    for e in entries.iter().rev() {
        if e.role == "user" {
            user_turns += 1;
            if user_turns > turns {
                break;
            }
            continue;
        }
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
            if name == "Read" {
                reads += 1;
                if reads >= threshold {
                    return true;
                }
            }
        }
    }
    false
}

fn strip_env_prefix(s: &str) -> &str {
    let mut rest = s;
    loop {
        let trimmed = rest.trim_start();
        let bytes = trimmed.as_bytes();
        if bytes.is_empty() {
            return rest;
        }
        if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
            return trimmed;
        }
        let mut i = 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return trimmed;
        }
        i += 1;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        rest = &trimmed[i..];
    }
}

fn render_envelope(provider: HookProvider, body: &str) -> String {
    match provider {
        HookProvider::Claude | HookProvider::Codex => serde_json::to_string(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": body,
            }
        }))
        .unwrap_or_default(),
        HookProvider::Opencode | HookProvider::Generic => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(role: &str, raw: Value) -> TranscriptEntry {
        TranscriptEntry {
            role: role.to_string(),
            text: String::new(),
            raw,
        }
    }

    fn bash_call(cmd: &str) -> Value {
        json!({
            "message": {
                "role": "assistant",
                "content": [{"type": "tool_use", "name": "Bash", "input": {"command": cmd}}]
            }
        })
    }

    fn read_call(path: &str) -> Value {
        json!({
            "message": {
                "role": "assistant",
                "content": [{"type": "tool_use", "name": "Read", "input": {"file_path": path}}]
            }
        })
    }

    fn user_msg() -> Value {
        json!({"message": {"role": "user", "content": "go"}})
    }

    #[test]
    fn empty_transcript_returns_empty() {
        assert_eq!(pick_tip(&[]), None);
    }

    #[test]
    fn unwrapped_cargo_test_triggers_snip_tip() {
        let entries = vec![entry("assistant", bash_call("cargo test --bins"))];
        assert_eq!(pick_tip(&entries), Some(TIP_SNIP));
    }

    #[test]
    fn already_wrapped_cargo_does_not_trigger_tip() {
        let entries = vec![entry("assistant", bash_call("codeterm snip cargo test"))];
        assert_eq!(pick_tip(&entries), None);
    }

    #[test]
    fn env_prefix_does_not_hide_noisy_call() {
        let entries = vec![entry("assistant", bash_call("RUST_LOG=debug cargo test"))];
        assert_eq!(pick_tip(&entries), Some(TIP_SNIP));
    }

    #[test]
    fn unrelated_bash_call_no_tip() {
        let entries = vec![entry("assistant", bash_call("ls -la"))];
        assert_eq!(pick_tip(&entries), None);
    }

    #[test]
    fn three_reads_within_5_user_turns_triggers_map_tip() {
        let entries = vec![
            entry("user", user_msg()),
            entry("assistant", read_call("a.rs")),
            entry("user", user_msg()),
            entry("assistant", read_call("b.rs")),
            entry("user", user_msg()),
            entry("assistant", read_call("c.rs")),
        ];
        assert_eq!(pick_tip(&entries), Some(TIP_MAP_HINT));
    }

    #[test]
    fn two_reads_no_tip() {
        let entries = vec![
            entry("user", user_msg()),
            entry("assistant", read_call("a.rs")),
            entry("user", user_msg()),
            entry("assistant", read_call("b.rs")),
        ];
        assert_eq!(pick_tip(&entries), None);
    }

    #[test]
    fn snip_tip_takes_precedence_over_map_tip() {
        let entries = vec![
            entry("user", user_msg()),
            entry("assistant", read_call("a.rs")),
            entry("assistant", read_call("b.rs")),
            entry("assistant", read_call("c.rs")),
            entry("assistant", bash_call("cargo test")),
        ];
        assert_eq!(pick_tip(&entries), Some(TIP_SNIP));
    }

    #[test]
    fn handle_emits_claude_additional_context() {
        let dir = std::env::temp_dir().join(format!("codeterm-ups-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let transcript = dir.join("t.jsonl");
        std::fs::write(
            &transcript,
            r#"{"role":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        )
        .unwrap();
        let input = format!(
            r#"{{"hook_event_name":"UserPromptSubmit","transcript_path":{:?}}}"#,
            transcript.to_string_lossy()
        );
        let out = handle(HookProvider::Claude, &input);
        let _ = std::fs::remove_dir_all(&dir);
        let v: Value = serde_json::from_str(&out).unwrap();
        let ac = v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(ac.contains("codeterm snip"));
        assert!(ac.contains("<system-reminder>"));
    }

    #[test]
    fn backticked_paths_pulled_out() {
        let prompt = "look at `src/lib.rs` and `Cargo.toml`, but ignore `not a path`";
        let paths = backticked_paths(prompt);
        assert_eq!(paths, vec!["src/lib.rs", "Cargo.toml"]);
    }

    #[test]
    fn backticked_paths_ignores_unbalanced() {
        let prompt = "look at `src/lib.rs and what about it";
        assert!(backticked_paths(prompt).is_empty());
    }

    #[test]
    fn looks_like_path_rejects_words_with_no_slash_or_dot() {
        assert!(!looks_like_path("hello"));
        assert!(looks_like_path("foo.rs"));
        assert!(looks_like_path("a/b"));
        assert!(!looks_like_path("file with space.rs"));
    }

    #[test]
    fn load_backticked_signatures_emits_for_existing_rust_file() {
        let dir = std::env::temp_dir().join(format!("codeterm-ups-bp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("foo.rs");
        std::fs::write(&f, "pub fn alpha() {}\npub struct Bar;\n").unwrap();
        let prompt = "describe `foo.rs` please";
        let out = load_backticked_signatures(prompt, Some(dir.to_str().unwrap())).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.contains("Signatures for paths"));
        assert!(out.contains("alpha"));
        assert!(out.contains("Bar"));
    }

    #[test]
    fn load_backticked_signatures_skips_missing_files() {
        let prompt = "see `nonexistent/abc.rs`";
        assert!(load_backticked_signatures(prompt, None).is_none());
    }

    #[test]
    fn load_backticked_signatures_skips_unsupported_extensions() {
        let dir = std::env::temp_dir().join(format!("codeterm-ups-ext-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("README.md");
        std::fs::write(&f, "# heading\n").unwrap();
        let prompt = "look at `README.md`";
        let out = load_backticked_signatures(prompt, Some(dir.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.is_none());
    }

    #[test]
    fn handle_combines_signatures_and_tip() {
        let dir = std::env::temp_dir().join(format!("codeterm-ups-comb-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("zeta.rs");
        std::fs::write(&f, "pub fn zeta() {}\n").unwrap();
        let transcript = dir.join("t.jsonl");
        std::fs::write(
            &transcript,
            r#"{"role":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        )
        .unwrap();
        let input = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": transcript.to_string_lossy(),
            "cwd": dir.to_string_lossy(),
            "prompt": "what does `zeta.rs` do?",
        })
        .to_string();
        let out = handle(HookProvider::Claude, &input);
        let _ = std::fs::remove_dir_all(&dir);
        let v: Value = serde_json::from_str(&out).unwrap();
        let ac = v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(ac.contains("zeta"));
        assert!(ac.contains("codeterm snip"));
    }

    #[test]
    fn handle_no_transcript_returns_empty() {
        assert_eq!(handle(HookProvider::Claude, "{}"), "");
    }

    #[test]
    fn handle_invalid_json_returns_empty() {
        assert_eq!(handle(HookProvider::Claude, "garbage"), "");
    }

    #[test]
    fn opencode_returns_raw_body() {
        let dir = std::env::temp_dir().join(format!("codeterm-ups-oc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let transcript = dir.join("t.jsonl");
        std::fs::write(
            &transcript,
            r#"{"role":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"bash","input":{"command":"go test ./..."}}]}}"#,
        )
        .unwrap();
        let input = format!(r#"{{"transcript_path":{:?}}}"#, transcript.to_string_lossy());
        let out = handle(HookProvider::Opencode, &input);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.contains("codeterm snip"));
        assert!(!out.contains("hookSpecificOutput"));
    }

    #[test]
    fn strip_env_prefix_basic() {
        assert_eq!(strip_env_prefix("FOO=bar cargo test"), "cargo test");
    }

    #[test]
    fn strip_env_prefix_multiple() {
        assert_eq!(
            strip_env_prefix("RUST_LOG=debug FOO=bar cargo test"),
            "cargo test"
        );
    }

    #[test]
    fn strip_env_prefix_none() {
        assert_eq!(strip_env_prefix("cargo test"), "cargo test");
    }
}
