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

    pub fn mark_changed(&self) {
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

    pub fn wait_until_changed(&self, generation: u64) -> bool {
        let mut state = self.state.lock();
        while !state.closed && state.generation == generation {
            self.cv.wait(&mut state);
        }
        !state.closed
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

mod content_hash;
mod registry;
pub use registry::{ScreenHandle, ScreenState};

#[derive(Default)]
pub struct PtyScreen {
    parser: Option<vt100::Parser>,
    input_mode: super::input_mode::InputMode,
    prompt_signal: Option<Arc<PromptSignal>>,
    shell_ready_signal: Option<Arc<ShellReadySignal>>,
    shell_ready_generation: Option<Option<u64>>,
    bare_prompt_signals: Vec<Arc<PromptSignal>>,
    agent_signal: Option<Arc<PromptSignal>>,
    screen_change_signal: Option<Arc<ScreenChangeSignal>>,
    screen_change_subscribers: Vec<Arc<ScreenChangeSignal>>,
    pty_generation: Option<u64>,
    content_hash: content_hash::ContentHash,
}

impl PtyScreen {
    fn mark_shell_ready_observed(&mut self) {
        self.shell_ready_generation = Some(self.pty_generation);
        if let Some(signal) = &self.shell_ready_signal {
            signal.mark_ready();
        }
    }

    pub fn process(&mut self, data: &[u8]) {
        self.input_mode.process(data);
        if data.windows(7).any(|window| window == b"\x1b]133;A")
            || data.windows(7).any(|window| window == b"\x1b]633;A")
        {
            self.mark_shell_ready_observed();
        }
        let parser = self
            .parser
            .get_or_insert_with(|| vt100::Parser::new(50, 120, 500));
        parser.process(data);
        self.content_hash.invalidate();
        let screen = parser.screen().contents();
        if screen_has_shell_readiness_prompt(&screen) {
            self.mark_shell_ready_observed();
        }
        if let Some(signal) = &self.prompt_signal {
            if !signal.is_ready() && screen_has_shell_prompt(&screen) {
                signal.mark_ready();
            }
        }
        if screen_has_bare_shell_prompt(&screen) {
            for signal in &self.bare_prompt_signals {
                signal.mark_ready();
            }
        }
        self.bare_prompt_signals.retain(|signal| !signal.is_ready());
        if let Some(signal) = &self.agent_signal {
            if !signal.is_ready() && screen_has_agent_tui(&screen) {
                signal.mark_ready();
            }
        }
        if let Some(signal) = &self.screen_change_signal {
            signal.mark_changed();
        }
        for signal in &self.screen_change_subscribers {
            signal.mark_changed();
        }
    }

    pub fn process_if_generation(&mut self, generation: u64, data: &[u8]) -> bool {
        if self.pty_generation != Some(generation) {
            return false;
        }
        self.process(data);
        true
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let parser = self
            .parser
            .get_or_insert_with(|| vt100::Parser::new(rows, cols, 500));
        parser.screen_mut().set_size(rows, cols);
        self.content_hash.invalidate();
    }

    pub fn get_screen_text(&self) -> Option<String> {
        self.parser.as_ref().map(|p| p.screen().contents())
    }

    pub fn get_screen_rows(&self) -> Option<Vec<String>> {
        let screen = self.parser.as_ref()?.screen();
        let (rows, cols) = screen.size();
        Some(
            (0..rows)
                .map(|row| {
                    screen
                        .contents_between(row, 0, row, cols)
                        .trim_end()
                        .to_string()
                })
                .collect(),
        )
    }

    pub fn win32_input_mode(&self) -> bool {
        self.input_mode.win32()
    }

    pub fn bracketed_paste_mode(&self) -> Option<bool> {
        self.parser
            .as_ref()
            .map(|parser| parser.screen().bracketed_paste())
    }

    pub fn reset_input_modes(&mut self) -> bool {
        let Some(parser) = &mut self.parser else {
            return false;
        };
        let (rows, cols) = parser.screen().size();
        let defaults = vt100::Parser::new(rows, cols, 500);
        let reset = defaults.screen().input_mode_diff(parser.screen());
        parser.process(&reset);
        self.content_hash.invalidate();
        true
    }
    pub fn native_prompt_input(&self) -> Option<crate::native_prompt::NativePromptInput> {
        self.parser.as_ref().map(|p| {
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
    pub fn cursor_input_state(&self) -> Option<(bool, bool, u16, u16)> {
        self.parser.as_ref().map(|p| {
            let screen = p.screen();
            let (rows, _cols) = screen.size();
            let (row, _col) = screen.cursor_position();
            (screen.alternate_screen(), !screen.hide_cursor(), row, rows)
        })
    }

    pub fn composer_input_state(&self) -> Option<ComposerInputState> {
        self.parser.as_ref().map(|parser| {
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
    pub fn get_screen_formatted(&self) -> Option<Vec<u8>> {
        self.parser.as_ref().map(|p| {
            let screen = p.screen();
            let mut buf = screen.contents_formatted();
            buf.extend_from_slice(&screen.cursor_state_formatted());
            buf
        })
    }

    pub fn get_content_hash(&mut self, exclude_bottom_rows: u16) -> Option<[u8; 32]> {
        Some(
            self.content_hash
                .get(self.parser.as_ref()?.screen(), exclude_bottom_rows),
        )
    }

    /// Trimmed per-row screen text plus the tail, derived from the existing
    /// per-pty vt100 parser — the same view `SignalExtractor` builds, but with no
    /// second parser. Feeds the live 3-state classifier's text-question /
    /// ready-prompt fingerprints (`screen_lines`) and picker-subtype fingerprints
    /// (`tail`). `tail_rows` anchors the tail to the last non-blank row.
    pub fn lines_and_tail(&self, tail_rows: usize) -> Option<(Vec<String>, String)> {
        let parser = self.parser.as_ref()?;
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
    pub fn tail_text(&self, max_rows: usize) -> Option<String> {
        let parser = self.parser.as_ref()?;
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

    pub fn arm_prompt_signal(&mut self) -> Arc<PromptSignal> {
        let signal = self
            .prompt_signal
            .get_or_insert_with(PromptSignal::new)
            .clone();
        if !signal.is_ready()
            && self
                .get_screen_text()
                .is_some_and(|screen| screen_has_shell_prompt(&screen))
        {
            signal.mark_ready();
        }
        signal
    }

    pub fn arm_bare_prompt_signal(&mut self) -> Arc<PromptSignal> {
        let signal = PromptSignal::new();
        self.bare_prompt_signals.push(signal.clone());
        if self
            .get_screen_text()
            .is_some_and(|screen| screen_has_bare_shell_prompt(&screen))
        {
            signal.mark_ready();
        }
        signal
    }

    pub fn arm_shell_ready_signal(&mut self) -> Arc<ShellReadySignal> {
        if self
            .shell_ready_signal
            .as_ref()
            .is_some_and(|signal| signal.is_closed())
        {
            self.shell_ready_signal = None;
        }
        let signal = self
            .shell_ready_signal
            .get_or_insert_with(ShellReadySignal::new)
            .clone();
        let observed = self.shell_ready_generation == Some(self.pty_generation);
        let on_screen = self
            .get_screen_text()
            .is_some_and(|screen| screen_has_shell_readiness_prompt(&screen));
        if on_screen {
            self.mark_shell_ready_observed();
        } else if observed {
            signal.mark_ready();
        }
        signal
    }

    pub fn has_pty_generation(&self) -> bool {
        self.pty_generation.is_some()
    }

    pub fn disarm_bare_prompt_signal(&mut self, signal: &Arc<PromptSignal>) {
        self.bare_prompt_signals
            .retain(|candidate| !Arc::ptr_eq(candidate, signal));
    }

    pub fn arm_agent_signal(&mut self) -> Arc<PromptSignal> {
        let signal = self
            .agent_signal
            .get_or_insert_with(PromptSignal::new)
            .clone();
        if !signal.is_ready()
            && self
                .get_screen_text()
                .is_some_and(|screen| screen_has_agent_tui(&screen))
        {
            signal.mark_ready();
        }
        signal
    }

    pub fn pty_generation(&self) -> Option<u64> {
        self.pty_generation
    }

    pub fn arm_screen_change_signal(&mut self) -> Arc<ScreenChangeSignal> {
        self.screen_change_signal
            .get_or_insert_with(ScreenChangeSignal::new)
            .clone()
    }

    pub fn bind_pty_generation(&mut self, generation: u64) -> bool {
        match self.pty_generation {
            Some(current) if current > generation => false,
            Some(current) if current == generation => true,
            Some(_) => {
                self.remove();
                self.pty_generation = Some(generation);
                true
            }
            None => {
                if self.shell_ready_generation == Some(None) {
                    self.shell_ready_generation = Some(Some(generation));
                }
                self.pty_generation = Some(generation);
                true
            }
        }
    }

    pub fn subscribe_screen_changes(&mut self) -> Arc<ScreenChangeSignal> {
        let signal = ScreenChangeSignal::new();
        self.screen_change_subscribers.push(signal.clone());
        signal
    }

    pub fn unsubscribe_screen_changes(&mut self, signal: &Arc<ScreenChangeSignal>) {
        self.screen_change_subscribers
            .retain(|active| !Arc::ptr_eq(active, signal));
        signal.close();
    }

    fn remove(&mut self) {
        if let Some(signal) = &self.prompt_signal {
            signal.mark_ready();
        }
        if let Some(signal) = &self.shell_ready_signal {
            signal.close();
        }
        for signal in &self.bare_prompt_signals {
            signal.mark_ready();
        }
        if let Some(signal) = &self.agent_signal {
            signal.mark_ready();
        }
        if let Some(signal) = &self.screen_change_signal {
            signal.close();
        }
        for signal in &self.screen_change_subscribers {
            signal.close();
        }
        *self = Self::default();
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

#[cfg(test)]
mod regression_tests;
