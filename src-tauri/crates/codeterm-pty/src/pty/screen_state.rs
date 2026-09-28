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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerInputState {
    pub alternate_screen: bool,
    pub cursor_visible: bool,
    pub cursor_row: u16,
    pub rows: u16,
    pub cols: u16,
    pub inverse_caret_row: Option<u16>,
    pub nonblank_inverse_cells: usize,
    pub sole_nonblank_inverse_row: Option<u16>,
    pub last_nonblank_row: Option<u16>,
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

    /// A closed signal can never become ready — its PTY generation went away.
    /// Callers log this distinctly so a torn-down wait is not read as a timeout.
    pub fn is_closed(&self) -> bool {
        self.state.lock().closed
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

/// Current VT100 screen grid per PTY; historical output remains in `OutputBuffer`.
/// Lines that scroll beyond the visible grid are not available through this state.
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

    fn mark_shell_ready_observed(&mut self, pty_id: &str) {
        self.shell_ready_generations.insert(
            pty_id.to_string(),
            self.pty_generations.get(pty_id).copied(),
        );
        if let Some(signal) = self.shell_ready_signals.get(pty_id) {
            signal.mark_ready();
        }
    }

    pub fn process(&mut self, pty_id: &str, data: &[u8]) {
        if data.windows(7).any(|window| window == b"\x1b]133;A")
            || data.windows(7).any(|window| window == b"\x1b]633;A")
        {
            self.mark_shell_ready_observed(pty_id);
        }
        let parser = self
            .parsers
            .entry(pty_id.to_string())
            .or_insert_with(|| vt100::Parser::new(50, 120, 500));
        parser.process(data);

        let screen = parser.screen().contents();

        if screen_has_shell_readiness_prompt(&screen) {
            self.mark_shell_ready_observed(pty_id);
        }

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
        if let Some(signals) = self.bare_prompt_signals.get_mut(pty_id) {
            signals.retain(|signal| !signal.is_ready());
        }
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

    pub fn get_screen_rows(&self, pty_id: &str) -> Option<Vec<String>> {
        let screen = self.parsers.get(pty_id)?.screen();
        let (rows, cols) = screen.size();
        Some(
            (0..rows)
                .map(|row| screen.contents_between(row, 0, row, cols).trim_end().to_string())
                .collect(),
        )
    }

    pub fn bracketed_paste_mode(&self, pty_id: &str) -> Option<bool> {
        self.parsers
            .get(pty_id)
            .map(|parser| parser.screen().bracketed_paste())
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

    pub fn composer_input_state(&self, pty_id: &str) -> Option<ComposerInputState> {
        self.parsers.get(pty_id).map(|parser| {
            let screen = parser.screen();
            let (rows, cols) = screen.size();
            let (cursor_row, _cursor_col) = screen.cursor_position();
            let inverse_cells = nonblank_inverse_cells(screen);
            let sole_nonblank_inverse_row = (inverse_cells.len() == 1).then(|| inverse_cells[0].0);
            ComposerInputState {
                alternate_screen: screen.alternate_screen(),
                cursor_visible: !screen.hide_cursor(),
                cursor_row,
                rows,
                cols,
                inverse_caret_row: hidden_inverse_caret_row(screen, &inverse_cells),
                nonblank_inverse_cells: inverse_cells.len(),
                sole_nonblank_inverse_row,
                last_nonblank_row: last_nonblank_row(screen),
            }
        })
    }

    /// Get screen as escape-coded bytes — preserves colors, cursor position, cursor visibility.
    /// Used for screen restore on tab remount (HMR, tab switch).
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

    /// Arm (or re-hand-out) the shell-ready signal. Readiness that already
    /// happened is not lost: a matching observation for this generation, or a
    /// prompt still rendered on screen, fires the signal at arm time.
    pub fn arm_shell_ready_signal(&mut self, pty_id: &str) -> Arc<ShellReadySignal> {
        if self
            .shell_ready_signals
            .get(pty_id)
            .is_some_and(|signal| signal.is_closed())
        {
            self.shell_ready_signals.remove(pty_id);
        }
        let signal = self
            .shell_ready_signals
            .entry(pty_id.to_string())
            .or_insert_with(ShellReadySignal::new)
            .clone();
        let current_generation = self.pty_generations.get(pty_id).copied();
        let observed =
            self.shell_ready_generations.get(pty_id).copied() == Some(current_generation);
        let on_screen = self
            .parsers
            .get(pty_id)
            .is_some_and(|parser| screen_has_shell_readiness_prompt(&parser.screen().contents()));
        if on_screen {
            self.mark_shell_ready_observed(pty_id);
        } else if observed {
            signal.mark_ready();
        }
        signal
    }

    /// Whether this PTY's output stream is already bound to a generation.
    /// Arming a readiness wait before the reader binds is the ordering that
    /// used to drop the wait entirely.
    pub fn has_pty_generation(&self, pty_id: &str) -> bool {
        self.pty_generations.contains_key(pty_id)
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

    pub fn pty_generation(&self, pty_id: &str) -> Option<u64> {
        self.pty_generations.get(pty_id).copied()
    }

    pub fn arm_screen_change_signal(&mut self, pty_id: &str) -> Arc<ScreenChangeSignal> {
        self.screen_change_signals
            .entry(pty_id.to_string())
            .or_insert_with(ScreenChangeSignal::new)
            .clone()
    }

    /// Bind this PTY's output stream to a generation. Replacing a generation
    /// tears down the previous one's state; the FIRST bind must not, or it
    /// closes waits armed by the launch thread before the reader started.
    pub fn bind_pty_generation(&mut self, pty_id: &str, generation: u64) -> bool {
        match self.pty_generations.get(pty_id).copied() {
            Some(current) if current > generation => false,
            Some(current) if current == generation => true,
            Some(_) => {
                self.remove(pty_id);
                self.pty_generations.insert(pty_id.to_string(), generation);
                true
            }
            None => {
                if self.shell_ready_generations.get(pty_id).copied() == Some(None) {
                    self.shell_ready_generations
                        .insert(pty_id.to_string(), Some(generation));
                }
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

fn nonblank_inverse_cells(screen: &vt100::Screen) -> Vec<(u16, u16)> {
    let (rows, cols) = screen.size();
    let mut inverse_cells = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if !cell.inverse() || cell.contents().trim().is_empty() {
                continue;
            }
            inverse_cells.push((row, col));
        }
    }
    inverse_cells
}

fn hidden_inverse_caret_row(screen: &vt100::Screen, inverse_cells: &[(u16, u16)]) -> Option<u16> {
    if !screen.hide_cursor() || inverse_cells.len() != 1 {
        return None;
    }
    Some(inverse_cells[0].0)
}

fn last_nonblank_row(screen: &vt100::Screen) -> Option<u16> {
    let (rows, cols) = screen.size();
    (0..rows).rev().find(|row| {
        (0..cols).any(|col| {
            screen
                .cell(*row, col)
                .is_some_and(|cell| !cell.contents().trim().is_empty())
        })
    })
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

/// Stricter than `screen_has_bare_shell_prompt`: proof the SHELL itself is
/// accepting input, used to gate writing a launch command. Agent composer
/// cursors (`❯` / `›`) and generic `word >` output are rejected — only real
/// POSIX (`$`/`%`), PowerShell (`PS …>`) and cmd (`C:\…>`) prompts count.
///
/// Reached through the last non-empty line, so a shell that prints a startup
/// banner first (macOS bash's zsh-migration notice) is still recognized.
pub fn screen_has_shell_readiness_prompt(screen_text: &str) -> bool {
    if screen_has_agent_tui(screen_text) {
        return false;
    }
    let Some(line) = screen_text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim_end)
    else {
        return false;
    };
    if line.contains('\u{276F}') || line.contains('\u{203A}') {
        return false;
    }
    line.ends_with('$')
        || line.ends_with('%')
        || (line.starts_with("PS ") && line.ends_with('>'))
        || (line.ends_with('>') && (line.contains(":\\") || line.contains(":/")))
}

#[cfg(test)]
mod tests;
