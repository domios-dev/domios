use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

/// Event-driven readiness signal, replacing the old busy-poll-on-screen pattern.
/// One instance per PTY; `wait` blocks until `mark_ready` fires or the timeout elapses.
#[derive(Default)]
pub struct PromptSignal {
    ready: Mutex<bool>,
    cv: Condvar,
}

impl PromptSignal {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_ready(&self) -> bool {
        *self.ready.lock()
    }

    pub fn mark_ready(&self) {
        let mut g = self.ready.lock();
        if !*g {
            *g = true;
            self.cv.notify_all();
        }
    }

    /// Block until ready or timeout. Returns the final readiness state.
    pub fn wait(&self, timeout: Duration) -> bool {
        let mut g = self.ready.lock();
        if *g {
            return true;
        }
        let _ = self.cv.wait_for(&mut g, timeout);
        *g
    }
}

/// Per-PTY VT100 screen parser state — tracks what the user sees on screen.
/// Separated from OutputBuffer to avoid lock contention: output processing
/// (which reads screen state) doesn't block chat view (which reads full history).
pub struct ScreenState {
    parsers: HashMap<String, vt100::Parser>,
    prompt_signals: HashMap<String, Arc<PromptSignal>>,
    agent_signals: HashMap<String, Arc<PromptSignal>>,
}

impl ScreenState {
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
            prompt_signals: HashMap::new(),
            agent_signals: HashMap::new(),
        }
    }

    pub fn process(&mut self, pty_id: &str, data: &[u8]) {
        let parser = self
            .parsers
            .entry(pty_id.to_string())
            .or_insert_with(|| vt100::Parser::new(50, 120, 500));
        parser.process(data);

        let screen = parser.screen().contents();

        if let Some(signal) = self.prompt_signals.get(pty_id) {
            if !signal.is_ready() && screen_has_shell_prompt(&screen) {
                signal.mark_ready();
            }
        }
        if let Some(signal) = self.agent_signals.get(pty_id) {
            if !signal.is_ready() && screen_has_agent_tui(&screen) {
                signal.mark_ready();
            }
        }
    }

    pub fn get_screen_text(&self, pty_id: &str) -> Option<String> {
        self.parsers.get(pty_id).map(|p| p.screen().contents())
    }

    pub fn native_prompt_input(
        &self,
        pty_id: &str,
    ) -> Option<crate::agent::native_prompt::NativePromptInput> {
        self.parsers.get(pty_id).map(|p| {
            let screen = p.screen();
            crate::agent::native_prompt::NativePromptInput {
                screen: crate::agent::attributed_screen::AttributedScreen::from_vt100(screen),
                cursor_hidden: screen.hide_cursor(),
                alt_screen: screen.alternate_screen(),
            }
        })
    }

    /// Get screen as escape-coded bytes — preserves colors, cursor position, cursor visibility.
    /// Used for screen restore on pane remount (HMR, tab switch).
    pub fn get_screen_formatted(&self, pty_id: &str) -> Option<Vec<u8>> {
        self.parsers.get(pty_id).map(|p| {
            let screen = p.screen();
            let mut buf = screen.contents_formatted();
            buf.extend_from_slice(&screen.cursor_state_formatted());
            buf
        })
    }

    pub fn get_content_hash(&self, pty_id: &str, exclude_bottom_rows: u16) -> Option<[u8; 32]> {
        use sha2::{Digest, Sha256};
        self.parsers.get(pty_id).map(|p| {
            let screen = p.screen();
            let (rows, cols) = screen.size();
            let end_row = rows.saturating_sub(exclude_bottom_rows);
            let mut h = Sha256::new();
            for row in 0..end_row {
                let row_text = screen.contents_between(row, 0, row, cols);
                let trimmed = row_text.trim();
                if !trimmed.is_empty() {
                    h.update(trimmed.as_bytes());
                }
            }
            h.finalize().into()
        })
    }

    /// Returns the last `max_rows` non-blank rows of the screen plus all rows
    /// after them, joined with `\n`. On a fresh PTY where content lives near
    /// the top and bottom rows are blank, this still returns the content.
    pub fn tail_text(&self, pty_id: &str, max_rows: usize) -> Option<String> {
        let parser = self.parsers.get(pty_id)?;
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let last_nonblank: Option<u16> = (0..rows)
            .rev()
            .find(|&r| !screen.contents_between(r, 0, r, cols).trim().is_empty());
        let end_row_excl = last_nonblank.map(|r| r + 1).unwrap_or(rows);
        let start_row = end_row_excl.saturating_sub(max_rows as u16);
        let mut out = String::new();
        for row in start_row..end_row_excl {
            let row_text = screen.contents_between(row, 0, row, cols);
            out.push_str(&row_text);
            out.push('\n');
        }
        Some(out)
    }

    /// Get or create the prompt-ready signal for a PTY. The first call arms detection:
    /// subsequent `process` calls will `mark_ready` as soon as the screen looks like a prompt.
    /// If a prompt is already on screen at arm-time, the signal fires immediately.
    pub fn arm_prompt_signal(&mut self, pty_id: &str) -> Arc<PromptSignal> {
        let signal = self
            .prompt_signals
            .entry(pty_id.to_string())
            .or_insert_with(PromptSignal::new)
            .clone();

        if !signal.is_ready() {
            if let Some(parser) = self.parsers.get(pty_id) {
                if screen_has_shell_prompt(&parser.screen().contents()) {
                    signal.mark_ready();
                }
            }
        }

        signal
    }

    /// Arm a one-shot signal that fires as soon as `screen_has_agent_tui` is true
    /// for this PTY. Fires immediately if a TUI is already on screen at arm-time.
    /// Mirrors `arm_prompt_signal`; callers block with `signal.wait(timeout)`.
    pub fn arm_agent_signal(&mut self, pty_id: &str) -> Arc<PromptSignal> {
        let signal = self
            .agent_signals
            .entry(pty_id.to_string())
            .or_insert_with(PromptSignal::new)
            .clone();

        if !signal.is_ready() {
            if let Some(parser) = self.parsers.get(pty_id) {
                if screen_has_agent_tui(&parser.screen().contents()) {
                    signal.mark_ready();
                }
            }
        }

        signal
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.parsers.remove(pty_id);
        if let Some(sig) = self.prompt_signals.remove(pty_id) {
            sig.mark_ready();
        }
        if let Some(sig) = self.agent_signals.remove(pty_id) {
            sig.mark_ready();
        }
    }
}

/// Heuristic: does the screen still show an agent TUI (Claude / Codex / Opencode)?
///
/// Looks for box-drawing glyphs (light rounded `╭╰╮╯` for Claude, heavy thick
/// `┃ ┏ ┓ ┗ ┛ ╹ ╻` for Opencode) plus distinctive footer phrases. Normal shell
/// output essentially never contains these; agent TUIs always do. Used by
/// `agent_restart` to avoid falsely concluding `/exit` succeeded — the TUI's
/// input cursor (`❯` / `›`) looks identical to many shell prompts.
pub fn screen_has_agent_tui(screen_text: &str) -> bool {
    // Box-drawing corners + thick verticals seen in agent TUIs but never in shells.
    const TUI_GLYPHS: &[char] = &[
        '╭', '╰', '╮', '╯', // Claude (light rounded)
        '┏', '┓', '┗', '┛', // heavy square corners
        '╹', '╻', '╺', '╸', // half-line caps (opencode logo)
        '┃', // heavy vertical (opencode borders)
    ];
    if screen_text.chars().any(|c| TUI_GLYPHS.contains(&c)) {
        return true;
    }
    // Six+ consecutive `▀` (upper-half blocks) — opencode logo/borders never appear in normal shell output.
    screen_text.contains("▀▀▀▀▀▀")
}

/// Like `screen_has_shell_prompt` but rejects screens where an agent TUI is
/// still visible. Use this when you need to confirm the shell is back after
/// killing an agent — not for general "input ready" checks.
pub fn screen_has_bare_shell_prompt(screen_text: &str) -> bool {
    !screen_has_agent_tui(screen_text) && screen_has_shell_prompt(screen_text)
}

/// Heuristic: does the last non-empty line of screen text look like a shell prompt?
///
/// Rejects PowerShell continuation prompts (`>>`, `>>>`) which are NOT
/// ready-for-input — they appear when the previous command was incomplete
/// (unclosed quote, mid-pipe, etc). Sending a launch command into that state
/// dumps the literal text as further input instead of starting the agent.
///
/// Also recognizes `❯` (U+276F) used by Claude Code TUI as an input cursor.
pub fn screen_has_shell_prompt(screen_text: &str) -> bool {
    let last_line = screen_text.lines().rev().find(|l| !l.trim().is_empty());
    match last_line {
        None => false,
        Some(line) => {
            let trimmed = line.trim_end();
            // PowerShell continuation: `>>`, `>> `, `>>>`, etc. NEVER ready.
            if trimmed.trim_start().chars().all(|c| c == '>') {
                return false;
            }
            // Bare `>` alone is also a continuation indicator (cmd.exe / sh heredoc); reject.
            if trimmed == ">" || trimmed == "> " {
                return false;
            }
            // Claude Code TUI idle input cursor: `❯` (U+276F), optional trailing space. Some agent TUIs use `›` (U+203A) similarly. Match anywhere on the
            // last line so this still fires when the cursor sits inside a box.
            if trimmed.contains('\u{276F}') || trimmed.contains('\u{203A}') {
                return true;
            }
            trimmed.ends_with("$ ")
                || trimmed.ends_with('$')
                || trimmed.ends_with("% ")
                || trimmed.ends_with('%')
                // PowerShell ready prompt: starts with "PS " and ends with ">".
                || (trimmed.starts_with("PS ") && trimmed.ends_with('>'))
                // Generic `name>` form (cmd.exe `C:\path>`, fish `name>`, etc). Must contain non-`>` content before the trailing `>` so we don't accept bare-arrow continuation lines.
                || (trimmed.ends_with('>') && trimmed.chars().any(|c| c != '>' && !c.is_whitespace()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_continuation_is_not_a_ready_prompt() {
        // The `>>` continuation indicator appears when the previous command was incomplete (unclosed quote, mid-pipe). NEVER treat as ready — the
        // launch_cmd would just become more input on the broken line.
        assert!(!screen_has_shell_prompt(">>"));
        assert!(!screen_has_shell_prompt(">> "));
        assert!(!screen_has_shell_prompt(">>>"));
        assert!(!screen_has_shell_prompt(">"));
        assert!(!screen_has_shell_prompt("> "));
    }

    #[test]
    fn powershell_ready_prompt_is_recognized() {
        assert!(screen_has_shell_prompt("PS C:\\Users\\User>"));
        assert!(screen_has_shell_prompt("PS C:\\Developer\\codeterm> "));
    }

    #[test]
    fn cmd_and_unix_prompts_are_recognized() {
        assert!(screen_has_shell_prompt("C:\\Users\\User>"));
        assert!(screen_has_shell_prompt("user@host:~$ "));
        assert!(screen_has_shell_prompt("user@host:~%"));
    }

    #[test]
    fn claude_tui_input_cursor_is_recognized() {
        // Issue #72: idle Claude TUI shows `❯` (U+276F) as the input cursor. `send --verify` was timing out with PromptNeverReady because the
        // matcher only knew POSIX/Windows shell markers.
        assert!(screen_has_shell_prompt("❯ "));
        assert!(screen_has_shell_prompt("❯"));
        // Cursor inside a box border (typical Claude TUI layout).
        assert!(screen_has_shell_prompt(
            "│ ❯                                        │"
        ));
        // Some agent TUIs use the lighter single-arrow glyph.
        assert!(screen_has_shell_prompt("› hello"));
        // Mid-action TUI screens must still report not-ready.
        assert!(!screen_has_shell_prompt("working..."));
        assert!(!screen_has_shell_prompt("Esc to interrupt"));
    }

    #[test]
    fn bare_shell_prompt_rejects_active_tui() {
        // Claude TUI with idle cursor on the last non-empty line. `screen_has_shell_prompt` accepts `❯` (issue #72), so without the TUI veto the
        // restart code would think the shell is back and pipe the launch command into Claude's input box.
        let claude_tui = "╭──────────────────────────╮\n│ ❯                        │";
        assert!(
            screen_has_shell_prompt(claude_tui),
            "❯ still looks shell-ish"
        );
        assert!(
            !screen_has_bare_shell_prompt(claude_tui),
            "TUI border must veto"
        );
    }

    #[test]
    fn bare_shell_prompt_accepts_real_shell() {
        // Starship-style prompt — `❯` but no box-drawing or TUI footer.
        assert!(screen_has_bare_shell_prompt(
            "~/code/codeterm on  master\n❯ "
        ));
        assert!(screen_has_bare_shell_prompt("user@host:~$ "));
        assert!(screen_has_bare_shell_prompt("PS C:\\Users\\User> "));
    }

    #[test]
    fn agent_tui_detected_by_universal_glyphs() {
        // Universal glyph check (box-drawing, block chars) — no text patterns here. Text patterns live in each provider's screen_has_tui()
        // implementation.
        assert!(screen_has_agent_tui("╭─ foo ─╮"));
        assert!(screen_has_agent_tui("▀▀▀▀▀▀ opencode logo"));
        assert!(!screen_has_agent_tui("some content\n? for shortcuts")); // text → provider
        assert!(!screen_has_agent_tui("working\nEsc to interrupt")); // text → provider
        assert!(!screen_has_agent_tui("just a plain shell output"));
        assert!(!screen_has_agent_tui("❯ "));
    }

    #[test]
    fn test_empty_state_returns_none() {
        let ss = ScreenState::new();
        assert!(ss.get_screen_text("nonexistent").is_none());
        assert!(ss.get_screen_formatted("nonexistent").is_none());
        assert!(ss.get_content_hash("nonexistent", 0).is_none());
    }

    #[test]
    fn test_process_and_get_text() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"Hello, world!");
        let text = ss.get_screen_text("pty1").unwrap();
        assert!(text.contains("Hello, world!"));
    }

    #[test]
    fn test_process_overwrites_at_cursor() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"ABCDE\rXY");
        let text = ss.get_screen_text("pty1").unwrap();
        assert!(text.contains("XYCDE"));
    }

    #[test]
    fn test_formatted_preserves_cursor_hide() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"Hello\x1b[?25l");
        let formatted = ss.get_screen_formatted("pty1").unwrap();
        let s = String::from_utf8_lossy(&formatted);
        assert!(
            s.contains("\x1b[?25l"),
            "formatted should preserve DECTCEM hide: {:?}",
            s
        );
    }

    #[test]
    fn test_formatted_preserves_colors() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"\x1b[31mRed text\x1b[0m");
        let formatted = ss.get_screen_formatted("pty1").unwrap();
        assert!(!formatted.is_empty());
        let s = String::from_utf8_lossy(&formatted);
        assert!(s.contains("\x1b["), "formatted should contain escape codes");
        assert!(s.contains("Red text"));
    }

    #[test]
    fn test_content_hash_changes_with_content() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"Content A");
        let hash1 = ss.get_content_hash("pty1", 0).unwrap();

        ss.process("pty1", b"\x1b[2J\x1b[HContent B");
        let hash2 = ss.get_content_hash("pty1", 0).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_excludes_bottom_rows() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"Top line\x1b[50;1HBottom line");
        let hash_all = ss.get_content_hash("pty1", 0).unwrap();
        let hash_no_bottom = ss.get_content_hash("pty1", 3).unwrap();
        assert_ne!(hash_all, hash_no_bottom);
    }

    #[test]
    fn test_remove_cleans_up() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"data");
        assert!(ss.get_screen_text("pty1").is_some());
        ss.remove("pty1");
        assert!(ss.get_screen_text("pty1").is_none());
    }

    #[test]
    fn test_multiple_ptys_isolated() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"First");
        ss.process("pty2", b"Second");
        assert!(ss.get_screen_text("pty1").unwrap().contains("First"));
        assert!(ss.get_screen_text("pty2").unwrap().contains("Second"));
        assert!(!ss.get_screen_text("pty1").unwrap().contains("Second"));
    }

    #[test]
    fn test_prompt_signal_fires_on_bash_prompt() {
        let mut ss = ScreenState::new();
        let sig = ss.arm_prompt_signal("pty1");
        assert!(!sig.is_ready());
        ss.process("pty1", b"user@host:~$ ");
        assert!(sig.is_ready());
    }

    #[test]
    fn test_prompt_signal_fires_on_powershell_prompt() {
        let mut ss = ScreenState::new();
        let sig = ss.arm_prompt_signal("pty1");
        ss.process("pty1", b"PS C:\\Developer> ");
        assert!(sig.is_ready());
    }

    #[test]
    fn test_prompt_signal_fires_immediately_if_prompt_already_present() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"user@host:~$ ");
        let sig = ss.arm_prompt_signal("pty1");
        assert!(sig.is_ready());
    }

    #[test]
    fn test_prompt_signal_not_fired_on_plain_output() {
        let mut ss = ScreenState::new();
        let sig = ss.arm_prompt_signal("pty1");
        ss.process("pty1", b"some regular line without prompt marker");
        assert!(!sig.is_ready());
    }

    #[test]
    fn test_prompt_signal_wait_returns_true_on_ready() {
        let mut ss = ScreenState::new();
        let sig = ss.arm_prompt_signal("pty1");
        let sig2 = sig.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            sig2.mark_ready();
        });
        let got = sig.wait(Duration::from_secs(1));
        handle.join().unwrap();
        assert!(got);
    }

    #[test]
    fn test_prompt_signal_wait_returns_false_on_timeout() {
        let sig = PromptSignal::new();
        let got = sig.wait(Duration::from_millis(30));
        assert!(!got);
    }
}
