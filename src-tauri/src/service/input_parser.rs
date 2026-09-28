//! PTY input parser — maps typed launch commands to `agent_type`.
//!
//! Hooks into the PTY write path. When a user types `claude ...<Enter>` or
//! `codex ...<Enter>` (or any registered provider's `binary_name()`) directly
//! in a shell tab, the line buffer captures bytes between newlines and on
//! Enter resolves the command token to a provider id. Treated as explicit user
//! intent — locks `agent_type` so output-based detection can't override it.
//!
//! Locked tabs are skipped entirely. Already-typed lines aren't re-processed
//! after a tab locks; we just drop the buffer.

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

    /// Process raw bytes about to be written to the PTY behind `tab_id`. Returns
    /// the provider id when a launch command was just recognised (Enter pressed,
    /// command matched a registered `binary_name()`); `None` otherwise.
    pub fn process(&self, tab_id: &str, bytes: &[u8]) -> Option<String> {
        if tab_id.is_empty() || bytes.is_empty() {
            return None;
        }
        let mut buffers = self.buffers.lock();
        let state = buffers.entry(tab_id.to_string()).or_default();
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

    pub fn clear(&self, tab_id: &str) {
        self.buffers.lock().remove(tab_id);
    }

    fn resolve_line(&self, line: &str) -> Option<String> {
        let tokens = shell_tokens(line);
        let token = command_token(&tokens)?;
        let basename = token
            .rsplit_once(|c| c == '/' || c == '\\')
            .map(|(_, name)| name)
            .unwrap_or(&token);
        let basename = basename
            .rsplit_once('.')
            .filter(|(_, extension)| extension.eq_ignore_ascii_case("exe"))
            .map(|(stem, _)| stem)
            .unwrap_or(basename)
            .to_ascii_lowercase();
        if basename.is_empty() {
            return None;
        }
        self.registry.as_ref()?.provider_for_binary(&basename)
    }
}

fn shell_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            token.push(character);
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            if characters
                .peek()
                .is_some_and(|next| matches!(next, ' ' | '\t' | '\\' | '"' | '\''))
            {
                escaped = true;
            } else {
                token.push(character);
            }
        } else if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if quote.is_none() && character.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(character);
        }
    }
    if escaped {
        token.push('\\');
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn command_token(tokens: &[String]) -> Option<String> {
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        if is_environment_assignment(token) {
            index += 1;
        } else if token == "sudo" || token == "env" {
            index = skip_wrapper_options(tokens, index + 1, token == "sudo");
        } else if matches!(token.as_str(), "npx" | "pnpm" | "yarn") {
            return launcher_target(tokens, index + 1, token);
        } else if token == "npm" && tokens.get(index + 1).is_some_and(|next| next == "exec") {
            return launcher_target(tokens, index + 2, token);
        } else {
            return Some(token.clone());
        }
    }
    None
}

fn skip_wrapper_options(tokens: &[String], mut index: usize, sudo: bool) -> usize {
    while let Some(token) = tokens.get(index) {
        if is_environment_assignment(token) || (!sudo && token.starts_with('-')) {
            index += 1;
        } else if sudo && matches!(token.as_str(), "-u" | "-g" | "-h" | "-p" | "-C" | "-T") {
            index += 2;
        } else if sudo && token.starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }
    index
}

fn launcher_target(tokens: &[String], mut index: usize, launcher: &str) -> Option<String> {
    if matches!(launcher, "pnpm" | "yarn") {
        if !matches!(tokens.get(index).map(String::as_str), Some("dlx" | "exec")) {
            return None;
        }
        index += 1;
    }
    while let Some(token) = tokens.get(index) {
        if token == "--" {
            index += 1;
            break;
        }
        if !token.starts_with('-') {
            return Some(token.clone());
        }
        index += 1;
    }
    tokens.get(index).cloned()
}

fn is_environment_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
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
        fn id(&self) -> &str {
            self.id
        }
        fn display_name(&self) -> &str {
            self.id
        }
        fn binary_name(&self) -> &str {
            self.binary
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::from_secs(0),
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
    }

    fn registry() -> Arc<AgentRegistry> {
        let r = AgentRegistry::new();
        r.register_provider(Box::new(FakeProvider {
            id: "claude",
            binary: "claude",
        }));
        r.register_provider(Box::new(FakeProvider {
            id: "codex",
            binary: "codex",
        }));
        r.register_provider(Box::new(FakeProvider {
            id: "opencode",
            binary: "opencode",
        }));
        Arc::new(r)
    }

    #[test]
    fn matches_simple_command_with_flags() {
        let p = InputParser::new(registry());
        assert_eq!(
            p.process("tab1", b"codex --foo\r").as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn matches_relative_path_prefix() {
        let p = InputParser::new(registry());
        assert_eq!(
            p.process("tab1", b"./codex resume xyz\r").as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn matches_absolute_path_prefix() {
        let p = InputParser::new(registry());
        assert_eq!(
            p.process("tab1", b"/usr/local/bin/claude\n").as_deref(),
            Some("claude"),
        );
    }

    #[test]
    fn does_not_match_arg_position() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"echo codex\r").is_none());
    }

    #[test]
    fn provider_names_only_in_arguments_do_not_assign_identity() {
        let p = InputParser::new(registry());
        for command in [
            "cat codex-notes.txt",
            "cat /tmp/codex-retry-screen.txt",
            "grep claude file",
            "vim opencode.md",
            "echo codex",
            "printf '%s' 'claude'",
        ] {
            let line = format!("{command}\r");
            assert!(p.process("tab1", line.as_bytes()).is_none(), "{command}");
        }
    }

    #[test]
    fn provider_launchers_are_detected_in_program_position() {
        let p = InputParser::new(registry());
        for (command, provider) in [
            ("codex", "codex"),
            ("claude --resume session", "claude"),
            ("opencode", "opencode"),
            ("/usr/bin/codex", "codex"),
            (r#""C:\tools\claude.exe" --resume session"#, "claude"),
            ("CODEX_HOME=/tmp/codex codex", "codex"),
            ("sudo -u agent /usr/bin/claude", "claude"),
            ("npx @openai/codex", "codex"),
            ("npm exec -- @openai/codex", "codex"),
            ("pnpm dlx @openai/codex", "codex"),
        ] {
            let line = format!("{command}\r");
            assert_eq!(
                p.process("tab1", line.as_bytes()).as_deref(),
                Some(provider)
            );
        }
    }

    #[test]
    fn does_not_match_unknown_binary() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"ls -la\r").is_none());
    }

    #[test]
    fn handles_multi_byte_writes() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"cod").is_none());
        assert!(p.process("tab1", b"ex --bar").is_none());
        assert_eq!(p.process("tab1", b"\r").as_deref(), Some("codex"));
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
        assert_eq!(p.process("tab1", &bytes).as_deref(), Some("codex"));
    }

    #[test]
    fn ctrl_c_clears_buffer() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"codex").is_none());
        assert!(p.process("tab1", &[0x03]).is_none());
        assert!(p.process("tab1", b"ls\r").is_none());
    }

    #[test]
    fn newline_alone_does_not_match() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"\r").is_none());
        assert!(p.process("tab1", b"\n").is_none());
    }

    #[test]
    fn long_line_does_not_match_then_recovers() {
        let p = InputParser::new(registry());
        // Overflow without newline → buffer is poisoned. Even if the bytes happen to start with "codex", the line never resolves on the next Enter because we lost data.
        let mut huge = Vec::new();
        huge.extend_from_slice(b"codex");
        huge.extend(std::iter::repeat(b'a').take(MAX_BUFFERED_LINE));
        huge.push(b'\r');
        assert!(
            p.process("tab1", &huge).is_none(),
            "poisoned line must not match"
        );
        // After the \r resets, fresh codex<Enter> resolves cleanly.
        assert_eq!(p.process("tab1", b"codex\r").as_deref(), Some("codex"));
    }

    #[test]
    fn per_tab_isolation() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"cod").is_none());
        assert!(p.process("tab2", b"codex\r").is_some());
        assert_eq!(p.process("tab1", b"ex\r").as_deref(), Some("codex"));
    }

    #[test]
    fn empty_tab_id_no_op() {
        let p = InputParser::new(registry());
        assert!(p.process("", b"codex\r").is_none());
    }

    #[test]
    fn clear_drops_buffer() {
        let p = InputParser::new(registry());
        assert!(p.process("tab1", b"cod").is_none());
        p.clear("tab1");
        assert!(p.process("tab1", b"ex\r").is_none());
    }
}
