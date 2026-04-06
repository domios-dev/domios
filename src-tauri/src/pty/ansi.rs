//! Pure ANSI/terminal-output helpers.
//!
//! Three complementary strippers:
//! - `clean_terminal_output` — full VT100 emulation, reflows to a virtual screen.
//!   Correct for "show the user what the terminal would render" (spinners, cursor moves).
//! - `strip_ansi` — light escape-code scrubber, preserves text layout. Use for
//!   structural pattern matching where reflow would corrupt positions.
//! - `strip_ansi_codes` — legacy fast path, similar to `strip_ansi` but also
//!   drops `\r`. Kept for backwards compatibility with existing call sites.

pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii_alphabetic() || ch == '~' { break; }
                    }
                } else if next == ']' {
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\x07' { break; }
                        if ch == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                } else {
                    chars.next();
                }
            }
        } else if c == '\r' {
            continue;
        } else {
            result.push(c);
        }
    }
    result
}

pub fn clean_terminal_output(raw: &str) -> String {
    let mut parser = vt100::Parser::new(500, 200, 10000);
    parser.process(raw.as_bytes());
    parser.screen().contents()
}

pub fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc.is_ascii_alphabetic() || nc == '@' || nc == '~' { break; }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\x07' { break; }
                        if nc == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                }
                Some('(' | ')') => { chars.next(); chars.next(); }
                _ => { chars.next(); }
            }
        } else if c == '\r' {
            // Skip CR, keep LF
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_codes_plain_text() {
        assert_eq!(strip_ansi_codes("hello world"), "hello world");
    }

    #[test]
    fn strip_ansi_codes_csi() {
        assert_eq!(strip_ansi_codes("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_codes_osc_bel() {
        assert_eq!(strip_ansi_codes("\x1b]0;title\x07text"), "text");
    }

    #[test]
    fn strip_ansi_codes_osc_st() {
        assert_eq!(strip_ansi_codes("\x1b]0;title\x1b\\text"), "text");
    }

    #[test]
    fn strip_ansi_codes_drops_cr() {
        assert_eq!(strip_ansi_codes("line1\r\nline2"), "line1\nline2");
    }

    #[test]
    fn strip_ansi_codes_multiple_sequences() {
        assert_eq!(
            strip_ansi_codes("\x1b[1;34mBold Blue\x1b[0m and \x1b[31mRed\x1b[0m"),
            "Bold Blue and Red"
        );
    }

    #[test]
    fn strip_ansi_codes_empty() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn strip_ansi_codes_cursor_motion() {
        assert_eq!(strip_ansi_codes("\x1b[2Aup\x1b[3Bdown"), "updown");
    }

    #[test]
    fn strip_ansi_plain_text() {
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn strip_ansi_csi_color() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strip_ansi_preserves_newlines_drops_cr() {
        assert_eq!(strip_ansi("a\r\nb"), "a\nb");
    }

    #[test]
    fn strip_ansi_osc_title() {
        assert_eq!(strip_ansi("\x1b]0;title\x07body"), "body");
    }

    #[test]
    fn strip_ansi_charset_designator() {
        // ESC ( B selects ASCII, should be consumed without leaking
        assert_eq!(strip_ansi("\x1b(Btext"), "text");
    }

    #[test]
    fn clean_terminal_output_basic_text() {
        let out = clean_terminal_output("hello");
        assert!(out.contains("hello"));
    }

    #[test]
    fn clean_terminal_output_strips_colors() {
        let out = clean_terminal_output("\x1b[32mgreen\x1b[0m");
        assert!(out.contains("green"));
        assert!(!out.contains("\x1b"));
    }

    #[test]
    fn clean_terminal_output_handles_overwrites() {
        // Write "abc", then CR + overwrite with "xy" — terminal shows "xyc"
        let out = clean_terminal_output("abc\rxy");
        let first_line = out.lines().next().unwrap_or("");
        assert!(first_line.starts_with("xyc"));
    }
}
