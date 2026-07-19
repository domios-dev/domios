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

#[derive(Default)]
struct ScreenChangeState {
    generation: u64,
    closed: bool,
}

#[derive(Default)]
pub struct ScreenChangeSignal {
    state: Mutex<ScreenChangeState>,
    cv: Condvar,
}

impl ScreenChangeSignal {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn generation(&self) -> u64 {
        self.state.lock().generation
    }

    fn mark_changed(&self) {
        let mut state = self.state.lock();
        state.generation = state.generation.wrapping_add(1);
        self.cv.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        self.cv.notify_all();
    }

    pub fn wait_after(&self, generation: u64, timeout: Duration) -> bool {
        let mut state = self.state.lock();
        if state.generation != generation {
            return true;
        }
        if state.closed {
            return false;
        }
        let _ = self.cv.wait_for(&mut state, timeout);
        !state.closed && state.generation != generation
    }
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

#[derive(Default)]
struct ShellReadyState {
    ready: bool,
    closed: bool,
}

#[derive(Default)]
pub struct ShellReadySignal {
    state: Mutex<ShellReadyState>,
    cv: Condvar,
}

impl ShellReadySignal {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_ready(&self) -> bool {
        self.state.lock().ready
    }

    fn mark_ready(&self) {
        let mut state = self.state.lock();
        if !state.ready && !state.closed {
            state.ready = true;
            self.cv.notify_all();
        }
    }

    fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        self.cv.notify_all();
    }

    pub fn wait(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock();
        if state.ready {
            return true;
        }
        if state.closed {
            return false;
        }
        let _ = self.cv.wait_for(&mut state, timeout);
        state.ready
    }
}

/// Per-PTY VT100 screen parser state — tracks what the user sees on screen.
/// Separated from OutputBuffer to avoid lock contention: output processing
/// (which reads screen state) doesn't block chat view (which reads full history).
pub struct ScreenState {
    parsers: HashMap<String, vt100::Parser>,
    prompt_signals: HashMap<String, Arc<PromptSignal>>,
    shell_ready_signals: HashMap<String, Arc<ShellReadySignal>>,
    shell_ready_generations: HashMap<String, Option<u64>>,
    bare_prompt_signals: HashMap<String, Vec<Arc<PromptSignal>>>,
    agent_signals: HashMap<String, Arc<PromptSignal>>,
    screen_change_signals: HashMap<String, Arc<ScreenChangeSignal>>,
    screen_change_subscribers: HashMap<String, Vec<Arc<ScreenChangeSignal>>>,
    pty_generations: HashMap<String, u64>,
}

impl ScreenState {
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
            prompt_signals: HashMap::new(),
            shell_ready_signals: HashMap::new(),
            shell_ready_generations: HashMap::new(),
            bare_prompt_signals: HashMap::new(),
            agent_signals: HashMap::new(),
            screen_change_signals: HashMap::new(),
            screen_change_subscribers: HashMap::new(),
            pty_generations: HashMap::new(),
        }
    }

    pub fn process(&mut self, pty_id: &str, data: &[u8]) {
        if data.windows(7).any(|window| window == b"\x1b]133;A")
            || data.windows(7).any(|window| window == b"\x1b]633;A")
        {
            self.shell_ready_generations.insert(
                pty_id.to_string(),
                self.pty_generations.get(pty_id).copied(),
            );
            if let Some(signal) = self.shell_ready_signals.get(pty_id) {
                signal.mark_ready();
            }
        }
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
        if let Some(signals) = self.bare_prompt_signals.get(pty_id) {
            if screen_has_bare_shell_prompt(&screen) {
                for signal in signals {
                    signal.mark_ready();
                }
            }
        }
        self.bare_prompt_signals
            .get_mut(pty_id)
            .map(|signals| signals.retain(|signal| !signal.is_ready()));
        if let Some(signal) = self.agent_signals.get(pty_id) {
            if !signal.is_ready() && screen_has_agent_tui(&screen) {
                signal.mark_ready();
            }
        }
        if let Some(signal) = self.screen_change_signals.get(pty_id) {
            signal.mark_changed();
        }
        if let Some(subscribers) = self.screen_change_subscribers.get(pty_id) {
            for signal in subscribers {
                signal.mark_changed();
            }
        }
    }

    pub fn process_if_generation(&mut self, pty_id: &str, generation: u64, data: &[u8]) -> bool {
        if self.pty_generations.get(pty_id).copied() != Some(generation) {
            return false;
        }
        self.process(pty_id, data);
        true
    }

    pub fn resize(&mut self, pty_id: &str, cols: u16, rows: u16) {
        let parser = self
            .parsers
            .entry(pty_id.to_string())
            .or_insert_with(|| vt100::Parser::new(rows, cols, 500));
        parser.screen_mut().set_size(rows, cols);
    }

    pub fn get_screen_text(&self, pty_id: &str) -> Option<String> {
        self.parsers.get(pty_id).map(|p| p.screen().contents())
    }

    pub fn native_prompt_input(
        &self,
        pty_id: &str,
    ) -> Option<crate::native_prompt::NativePromptInput> {
        self.parsers.get(pty_id).map(|p| {
            let screen = p.screen();
            crate::native_prompt::NativePromptInput {
                screen: crate::attributed_screen::AttributedScreen::from_vt100(screen),
                cursor_hidden: screen.hide_cursor(),
                alt_screen: screen.alternate_screen(),
            }
        })
    }

    /// VT cursor geometry for the live composer-ready signal: `(alt_screen,
    /// cursor_visible, cursor_row, rows)`. Mirrors what the offline `SignalExtractor`
    /// exposes so the live and replay classifiers agree (see `composer_cursor_ready`).
    pub fn cursor_input_state(&self, pty_id: &str) -> Option<(bool, bool, u16, u16)> {
        self.parsers.get(pty_id).map(|p| {
            let screen = p.screen();
            let (rows, _cols) = screen.size();
            let (row, _col) = screen.cursor_position();
            (screen.alternate_screen(), !screen.hide_cursor(), row, rows)
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

    /// Trimmed per-row screen text plus the tail, derived from the existing
    /// per-pty vt100 parser — the same view `SignalExtractor` builds, but with no
    /// second parser. Feeds the live 3-state classifier's text-question /
    /// ready-prompt fingerprints (`screen_lines`) and picker-subtype fingerprints
    /// (`tail`). `tail_rows` anchors the tail to the last non-blank row.
    pub fn lines_and_tail(&self, pty_id: &str, tail_rows: usize) -> Option<(Vec<String>, String)> {
        let parser = self.parsers.get(pty_id)?;
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let mut lines = Vec::with_capacity(rows as usize);
        let mut last_nonblank: Option<usize> = None;
        for row in 0..rows {
            let text = screen.contents_between(row, 0, row, cols);
            let trimmed = text.trim_end().to_string();
            if !trimmed.is_empty() {
                last_nonblank = Some(row as usize);
            }
            lines.push(trimmed);
        }
        let end = last_nonblank.map_or(0, |r| r + 1);
        let start = end.saturating_sub(tail_rows);
        let tail = lines[start..end].join("\n");
        Some((lines, tail))
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

    /// Get or create the bare-shell prompt signal for a PTY. Unlike
    /// `arm_prompt_signal`, this rejects agent TUI composers that also use `❯`.
    pub fn arm_bare_prompt_signal(&mut self, pty_id: &str) -> Arc<PromptSignal> {
        let signal = PromptSignal::new();
        self.bare_prompt_signals
            .entry(pty_id.to_string())
            .or_default()
            .push(signal.clone());

        if !signal.is_ready() {
            if let Some(parser) = self.parsers.get(pty_id) {
                if screen_has_bare_shell_prompt(&parser.screen().contents()) {
                    signal.mark_ready();
                }
            }
        }

        signal
    }

    pub fn arm_shell_ready_signal(&mut self, pty_id: &str) -> Arc<ShellReadySignal> {
        let signal = self
            .shell_ready_signals
            .entry(pty_id.to_string())
            .or_insert_with(ShellReadySignal::new)
            .clone();
        let current_generation = self.pty_generations.get(pty_id).copied();
        if self.shell_ready_generations.get(pty_id).copied() == Some(current_generation) {
            signal.mark_ready();
        }
        signal
    }

    pub fn disarm_bare_prompt_signal(&mut self, pty_id: &str, signal: &Arc<PromptSignal>) {
        let remove_entry = self
            .bare_prompt_signals
            .get_mut(pty_id)
            .map(|signals| {
                signals.retain(|candidate| !Arc::ptr_eq(candidate, signal));
                signals.is_empty()
            })
            .unwrap_or(false);
        if remove_entry {
            self.bare_prompt_signals.remove(pty_id);
        }
    }

    #[cfg(test)]
    fn bare_prompt_waiter_count(&self, pty_id: &str) -> usize {
        self.bare_prompt_signals.get(pty_id).map_or(0, Vec::len)
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

    pub fn arm_screen_change_signal(&mut self, pty_id: &str) -> Arc<ScreenChangeSignal> {
        self.screen_change_signals
            .entry(pty_id.to_string())
            .or_insert_with(ScreenChangeSignal::new)
            .clone()
    }

    pub fn bind_pty_generation(&mut self, pty_id: &str, generation: u64) -> bool {
        match self.pty_generations.get(pty_id).copied() {
            Some(current) if current > generation => false,
            Some(current) if current == generation => true,
            _ => {
                self.remove(pty_id);
                self.pty_generations.insert(pty_id.to_string(), generation);
                true
            }
        }
    }

    pub fn remove_if_generation(&mut self, pty_id: &str, generation: u64) -> bool {
        if self.pty_generations.get(pty_id).copied() != Some(generation) {
            return false;
        }
        self.remove(pty_id);
        true
    }

    pub fn subscribe_screen_changes(&mut self, pty_id: &str) -> Arc<ScreenChangeSignal> {
        let signal = ScreenChangeSignal::new();
        self.screen_change_subscribers
            .entry(pty_id.to_string())
            .or_default()
            .push(signal.clone());
        signal
    }

    pub fn unsubscribe_screen_changes(&mut self, pty_id: &str, signal: &Arc<ScreenChangeSignal>) {
        let should_remove = self
            .screen_change_subscribers
            .get_mut(pty_id)
            .map(|subscribers| {
                subscribers.retain(|active| !Arc::ptr_eq(active, signal));
                subscribers.is_empty()
            })
            .unwrap_or(false);
        if should_remove {
            self.screen_change_subscribers.remove(pty_id);
        }
        signal.close();
    }

    pub fn screen_change_subscriber_count(&self) -> usize {
        self.screen_change_subscribers.values().map(Vec::len).sum()
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.pty_generations.remove(pty_id);
        self.parsers.remove(pty_id);
        self.shell_ready_generations.remove(pty_id);
        if let Some(sig) = self.prompt_signals.remove(pty_id) {
            sig.mark_ready();
        }
        if let Some(sig) = self.shell_ready_signals.remove(pty_id) {
            sig.close();
        }
        if let Some(signals) = self.bare_prompt_signals.remove(pty_id) {
            for signal in signals {
                signal.mark_ready();
            }
        }
        if let Some(sig) = self.agent_signals.remove(pty_id) {
            sig.mark_ready();
        }
        if let Some(signal) = self.screen_change_signals.remove(pty_id) {
            signal.close();
        }
        if let Some(subscribers) = self.screen_change_subscribers.remove(pty_id) {
            for signal in subscribers {
                signal.close();
            }
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
    fn cursor_input_state_clamps_a_composer_to_the_bottom_row() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"\x1b[999;1H\x1b[2m\xe2\x96\x8c\x1b[0m ");

        let (_alternate, visible, row, rows) = ss.cursor_input_state("pty1").unwrap();
        assert!(visible);
        assert!(
            row + 4 >= rows,
            "cursor row {row} must be near bottom {rows}"
        );
    }

    #[test]
    fn resize_keeps_cursor_geometry_in_sync_with_the_pty() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"\x1b[40;1H\x1b[2m\xe2\x96\x8c\x1b[0m ");
        ss.resize("pty1", 120, 40);

        let (_alternate, visible, row, rows) = ss.cursor_input_state("pty1").unwrap();
        assert!(visible);
        assert_eq!(rows, 40);
        assert!(
            row + 4 >= rows,
            "cursor row {row} must be near bottom {rows}"
        );
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
    fn bare_prompt_signal_rejects_agent_tui_prompt() {
        let mut ss = ScreenState::new();
        ss.process(
            "pty1",
            "╭──────────────────────────╮\n│ ❯                        │".as_bytes(),
        );
        let prompt = ss.arm_prompt_signal("pty1");
        let bare = ss.arm_bare_prompt_signal("pty1");
        assert!(
            prompt.is_ready(),
            "regular prompt signal accepts TUI composer"
        );
        assert!(
            !bare.is_ready(),
            "restore launch must not treat an agent composer as a bare shell"
        );

        ss.process("pty1", b"\x1b[2J\x1b[Huser@host:~$ ");
        assert!(
            bare.is_ready(),
            "bare prompt signal fires on real shell prompt"
        );
    }

    #[test]
    fn bare_prompt_signal_does_not_reuse_stale_ready_state() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"user@host:~$ ");
        let first = ss.arm_bare_prompt_signal("pty1");
        assert!(first.is_ready());

        ss.process(
            "pty1",
            "\x1b[2J\x1b[H╭──────────────────────────╮\n│ ❯                        │".as_bytes(),
        );
        let second = ss.arm_bare_prompt_signal("pty1");
        assert!(
            !second.is_ready(),
            "a previous bare shell prompt must not make a later TUI screen ready"
        );
    }

    #[test]
    fn shell_ready_requires_a_shell_integration_event() {
        let mut ss = ScreenState::new();
        let signal = ss.arm_shell_ready_signal("pty1");

        ss.process("pty1", b"ordinary output");
        assert!(!signal.is_ready());
        ss.process("pty1", b"\x1b]133;A\x07");
        assert!(signal.is_ready());
    }

    #[test]
    fn shell_ready_event_before_arm_is_remembered_for_current_generation() {
        let mut ss = ScreenState::new();
        assert!(ss.bind_pty_generation("pty1", 1));

        assert!(ss.process_if_generation("pty1", 1, b"\x1b]633;A\x07"));

        assert!(ss.arm_shell_ready_signal("pty1").is_ready());
    }

    #[test]
    fn raw_reader_generation_must_bind_before_output_is_accepted() {
        let mut ss = ScreenState::new();

        assert!(!ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
        assert!(ss.bind_pty_generation("pty1", 1));
        assert!(ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
        assert!(ss.arm_shell_ready_signal("pty1").is_ready());
    }

    #[test]
    fn late_shell_ready_event_from_old_generation_is_rejected() {
        let mut ss = ScreenState::new();
        assert!(ss.bind_pty_generation("pty1", 1));
        assert!(ss.bind_pty_generation("pty1", 2));

        assert!(!ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
        let signal = ss.arm_shell_ready_signal("pty1");

        assert!(!signal.is_ready());
        ss.process("pty1", b"ordinary output");
        assert!(!signal.is_ready());
        assert!(ss.process_if_generation("pty1", 2, b"\x1b]133;A\x07"));
        assert!(signal.is_ready());
    }

    #[test]
    fn replacing_pty_generation_cancels_armed_shell_ready_waiter() {
        let mut ss = ScreenState::new();
        assert!(ss.bind_pty_generation("pty1", 1));
        let signal = ss.arm_shell_ready_signal("pty1");

        assert!(ss.bind_pty_generation("pty1", 2));

        assert!(!signal.wait(Duration::ZERO));
    }

    #[test]
    fn shell_event_wakes_all_waiters_and_prunes_them() {
        let mut ss = ScreenState::new();
        let first = ss.arm_bare_prompt_signal("pty1");
        let second = ss.arm_bare_prompt_signal("pty1");

        ss.process("pty1", b"user@host:~$ ");

        assert!(first.is_ready());
        assert!(second.is_ready());
        assert_eq!(ss.bare_prompt_waiter_count("pty1"), 0);
    }

    #[test]
    fn disarmed_bare_prompt_waiter_does_not_accumulate_after_timeout() {
        let mut ss = ScreenState::new();
        let signal = ss.arm_bare_prompt_signal("pty1");

        ss.disarm_bare_prompt_signal("pty1", &signal);

        assert_eq!(ss.bare_prompt_waiter_count("pty1"), 0);
    }

    #[test]
    fn test_prompt_signal_wait_returns_true_on_ready() {
        let mut ss = ScreenState::new();
        let sig = ss.arm_prompt_signal("pty1");
        let sig2 = sig.clone();
        let handle = std::thread::spawn(move || {
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

    #[test]
    fn screen_change_signal_wakes_on_process_without_polling() {
        let mut state = ScreenState::new();
        let signal = state.arm_screen_change_signal("pty1");
        let generation = signal.generation();
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = signal.clone();
        let handle = std::thread::spawn(move || {
            tx.send(waiter.wait_after(generation, Duration::from_secs(1)))
                .unwrap();
        });

        state.process("pty1", b"agent frame");

        assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
        handle.join().unwrap();
    }

    #[test]
    fn unsubscribed_screen_change_signal_fails_waiters() {
        let mut state = ScreenState::new();
        let signal = state.subscribe_screen_changes("pty1");
        let generation = signal.generation();

        state.unsubscribe_screen_changes("pty1", &signal);

        assert!(!signal.wait_after(generation, Duration::ZERO));
        assert_eq!(state.screen_change_subscriber_count(), 0);
    }

    #[test]
    fn pty_teardown_closes_all_screen_change_subscribers() {
        let mut state = ScreenState::new();
        let first = state.subscribe_screen_changes("pty1");
        let second = state.subscribe_screen_changes("pty1");
        let first_generation = first.generation();
        let second_generation = second.generation();

        state.remove("pty1");

        assert!(!first.wait_after(first_generation, Duration::ZERO));
        assert!(!second.wait_after(second_generation, Duration::ZERO));
        assert_eq!(state.screen_change_subscriber_count(), 0);
    }

    #[test]
    fn old_generation_cannot_remove_replacement_screen() {
        let mut state = ScreenState::new();
        assert!(state.bind_pty_generation("pty1", 1));
        state.process("pty1", b"old frame");
        assert!(state.bind_pty_generation("pty1", 2));
        state.process("pty1", b"replacement frame");

        assert!(!state.remove_if_generation("pty1", 1));
        assert!(state.get_screen_text("pty1").is_some());
        assert!(state.remove_if_generation("pty1", 2));
        assert!(state.get_screen_text("pty1").is_none());
    }
}
