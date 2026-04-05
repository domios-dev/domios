use std::collections::HashMap;

/// Per-PTY VT100 screen parser state — tracks what the user sees on screen.
/// Separated from OutputBuffer to avoid lock contention: output processing
/// (which reads screen state) doesn't block chat view (which reads full history).
pub struct ScreenState {
    parsers: HashMap<String, vt100::Parser>,
}

impl ScreenState {
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
        }
    }

    pub fn process(&mut self, pty_id: &str, data: &[u8]) {
        let parser = self.parsers
            .entry(pty_id.to_string())
            .or_insert_with(|| vt100::Parser::new(50, 120, 500));
        parser.process(data);
    }

    pub fn get_screen_text(&self, pty_id: &str) -> Option<String> {
        self.parsers.get(pty_id).map(|p| p.screen().contents())
    }

    /// Get screen as escape-coded bytes — preserves colors, cursor position, cursor visibility.
    /// Used for screen restore on pane remount (HMR, tab switch).
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

    pub fn remove(&mut self, pty_id: &str) {
        self.parsers.remove(pty_id);
    }
}
