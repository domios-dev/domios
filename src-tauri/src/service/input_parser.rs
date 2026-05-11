//! PTY input parser — maps typed launch commands to `agent_type`.
//!
//! Hooks into the PTY write path. When a user types `claude ...<Enter>` or
//! `codex ...<Enter>` (or any registered provider's `binary_name()`) directly
//! in a shell pane, the line buffer captures bytes between newlines and on
//! Enter resolves the first token to a provider id. Treated as explicit user
//! intent — locks `agent_type` so output-based detection can't override it.
//!
//! Locked panes are skipped entirely. Already-typed lines aren't re-processed
//! after a pane locks; we just drop the buffer.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::agent::registry::AgentRegistry;

const MAX_BUFFERED_LINE: usize = 4096;

#[derive(Default)]
struct LineState {
    text: String,
    poisoned: bool,
}

#[derive(Default)]
pub struct InputParser {
    buffers: Mutex<HashMap<String, LineState>>,
    registry: Option<Arc<AgentRegistry>>,
}

impl InputParser {
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self {
            buffers: Mutex::new(HashMap::new()),
            registry: Some(registry),
        }
    }

    /// Process raw bytes about to be written to the PTY behind `pane_id`. Returns
    /// the provider id when a launch command was just recognised (Enter pressed,
    /// first token matched a registered `binary_name()`); `None` otherwise.
    pub fn process(&self, pane_id: &str, bytes: &[u8]) -> Option<String> {
        if pane_id.is_empty() || bytes.is_empty() {
            return None;
        }
        let mut buffers = self.buffers.lock();
        let state = buffers.entry(pane_id.to_string()).or_default();
        let mut matched: Option<String> = None;
        for &b in bytes {
            match b {
                b'\r' | b'\n' => {
                    if matched.is_none() && !state.poisoned {
                        matched = self.resolve_line(&state.text);
                    }
                    state.text.clear();
                    state.poisoned = false;
                }
                0x7f | 0x08 => {
                    state.text.pop();
                }
                0x03 | 0x04 | 0x15 => {
                    state.text.clear();
                    state.poisoned = false;
                }
                _ => {
                    if state.text.len() < MAX_BUFFERED_LINE {
                        state.text.push(b as char);
                    } else {
                        state.poisoned = true;
                    }
                }
            }
        }
        matched
    }

    pub fn clear(&self, pane_id: &str) {
        self.buffers.lock().remove(pane_id);
    }

    fn resolve_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let first = trimmed.split_whitespace().next()?;
        let token = first.trim_matches(|c: char| c == '"' || c == '\'');
        let basename = token.rsplit_once(|c| c == '/' || c == '\\')
            .map(|(_, name)| name)
            .unwrap_or(token);
        let basename = basename.to_ascii_lowercase();
        if basename.is_empty() {
            return None;
        }
        self.registry.as_ref()?.provider_for_binary(&basename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use std::path::PathBuf;
    use std::time::Duration;

    struct FakeProvider {
        id: &'static str,
        binary: &'static str,
    }
    impl AgentProvider for FakeProvider {
        fn id(&self) -> &str { self.id }
        fn display_name(&self) -> &str { self.id }
        fn binary_name(&self) -> &str { self.binary }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String { String::new() }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay { duration: Duration::from_secs(0) }
        }
        fn task_delivery(&self) -> TaskDelivery { TaskDelivery::PtyInput }
        fn detect_from_title(&self, _: &str) -> bool { false }
        fn detect_from_output(&self, _: &str) -> bool { false }
        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> { None }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> { None }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> { None }
    }

    fn registry() -> Arc<AgentRegistry> {
        let r = AgentRegistry::new();
        r.register_provider(Box::new(FakeProvider { id: "claude", binary: "claude" }));
        r.register_provider(Box::new(FakeProvider { id: "codex", binary: "codex" }));
        r.register_provider(Box::new(FakeProvider { id: "opencode", binary: "opencode" }));
        Arc::new(r)
    }

    #[test]
    fn matches_simple_command_with_flags() {
        let p = InputParser::new(registry());
        assert_eq!(p.process("pane1", b"codex --foo\r").as_deref(), Some("codex"));
    }

    #[test]
    fn matches_relative_path_prefix() {
        let p = InputParser::new(registry());
        assert_eq!(p.process("pane1", b"./codex resume xyz\r").as_deref(), Some("codex"));
    }

    #[test]
    fn matches_absolute_path_prefix() {
        let p = InputParser::new(registry());
        assert_eq!(
            p.process("pane1", b"/usr/local/bin/claude\n").as_deref(),
            Some("claude"),
        );
    }

    #[test]
    fn does_not_match_arg_position() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"echo codex\r").is_none());
    }

    #[test]
    fn does_not_match_unknown_binary() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"ls -la\r").is_none());
    }

    #[test]
    fn handles_multi_byte_writes() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"cod").is_none());
        assert!(p.process("pane1", b"ex --bar").is_none());
        assert_eq!(p.process("pane1", b"\r").as_deref(), Some("codex"));
    }

    #[test]
    fn backspace_erases_chars() {
        let p = InputParser::new(registry());
        // Type "codez", hit backspace twice, then "ex<Enter>"
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"codez");
        bytes.push(0x7f);
        bytes.push(0x7f);
        bytes.extend_from_slice(b"ex\r");
        assert_eq!(p.process("pane1", &bytes).as_deref(), Some("codex"));
    }

    #[test]
    fn ctrl_c_clears_buffer() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"codex").is_none());
        assert!(p.process("pane1", &[0x03]).is_none());
        assert!(p.process("pane1", b"ls\r").is_none());
    }

    #[test]
    fn newline_alone_does_not_match() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"\r").is_none());
        assert!(p.process("pane1", b"\n").is_none());
    }

    #[test]
    fn long_line_does_not_match_then_recovers() {
        let p = InputParser::new(registry());
        // Overflow without newline → buffer is poisoned. Even if the bytes
        // happen to start with "codex", the line never resolves on the next
        // Enter because we lost data.
        let mut huge = Vec::new();
        huge.extend_from_slice(b"codex");
        huge.extend(std::iter::repeat(b'a').take(MAX_BUFFERED_LINE));
        huge.push(b'\r');
        assert!(p.process("pane1", &huge).is_none(), "poisoned line must not match");
        // After the \r resets, fresh codex<Enter> resolves cleanly.
        assert_eq!(p.process("pane1", b"codex\r").as_deref(), Some("codex"));
    }

    #[test]
    fn per_pane_isolation() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"cod").is_none());
        assert!(p.process("pane2", b"codex\r").is_some());
        // pane1's partial line is untouched by pane2 activity
        assert_eq!(p.process("pane1", b"ex\r").as_deref(), Some("codex"));
    }

    #[test]
    fn empty_pane_id_no_op() {
        let p = InputParser::new(registry());
        assert!(p.process("", b"codex\r").is_none());
    }

    #[test]
    fn clear_drops_buffer() {
        let p = InputParser::new(registry());
        assert!(p.process("pane1", b"cod").is_none());
        p.clear("pane1");
        assert!(p.process("pane1", b"ex\r").is_none());
    }
}
