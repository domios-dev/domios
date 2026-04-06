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

#[cfg(test)]
mod tests {
    use super::*;

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
        // Write "ABCDE", move cursor to col 0, overwrite with "XY"
        ss.process("pty1", b"ABCDE\rXY");
        let text = ss.get_screen_text("pty1").unwrap();
        assert!(text.contains("XYCDE"));
    }

    #[test]
    fn test_formatted_preserves_cursor_hide() {
        let mut ss = ScreenState::new();
        // Write text then hide cursor (DECTCEM)
        ss.process("pty1", b"Hello\x1b[?25l");
        let formatted = ss.get_screen_formatted("pty1").unwrap();
        // Formatted output should contain the cursor hide sequence
        let s = String::from_utf8_lossy(&formatted);
        assert!(s.contains("\x1b[?25l"), "formatted should preserve DECTCEM hide: {:?}", s);
    }

    #[test]
    fn test_formatted_preserves_colors() {
        let mut ss = ScreenState::new();
        // Red text
        ss.process("pty1", b"\x1b[31mRed text\x1b[0m");
        let formatted = ss.get_screen_formatted("pty1").unwrap();
        assert!(!formatted.is_empty());
        // Should contain SGR sequences (color codes)
        let s = String::from_utf8_lossy(&formatted);
        assert!(s.contains("\x1b["), "formatted should contain escape codes");
        assert!(s.contains("Red text"));
    }

    #[test]
    fn test_content_hash_changes_with_content() {
        let mut ss = ScreenState::new();
        ss.process("pty1", b"Content A");
        let hash1 = ss.get_content_hash("pty1", 0).unwrap();

        ss.process("pty1", b"\x1b[2J\x1b[HContent B"); // clear screen + write
        let hash2 = ss.get_content_hash("pty1", 0).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_excludes_bottom_rows() {
        let mut ss = ScreenState::new();
        // Write on first line, then move to last line and write
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
}
