//! VT100 terminal emulation helpers for rendering PTY output as plain text.

pub fn clean_terminal_output(raw: &str) -> String {
    clean_terminal_output_sized(raw, 500, 200)
}

pub fn clean_terminal_output_sized(raw: &str, rows: u16, cols: u16) -> String {
    let mut parser = vt100::Parser::new(rows, cols, 10000);
    parser.process(raw.as_bytes());
    parser.screen().contents()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real 120-col macOS zsh capture: `printf '\033[31mMARKER_RED_TEST\033[0m'`
    // (no trailing newline), then zsh's partial-line %-marker + line redraw.
    const ZSH_120COL: &str = include_str!("../testdata/zsh_partial_line_120col.raw");

    #[test]
    fn partial_line_output_survives_at_matching_width() {
        let out = clean_terminal_output_sized(ZSH_120COL, 40, 120);
        assert!(
            out.contains("MARKER_RED_TEST"),
            "marker erased at real width: {out:?}"
        );
    }

    #[test]
    fn partial_line_output_erased_at_mismatched_width() {
        // Documents why call sites must pass the pty's real width: 200-col emulation of a 120-col stream never wraps zsh's %-padding, so the redraw erases the output line.
        let out = clean_terminal_output(ZSH_120COL);
        assert!(
            !out.contains("MARKER_RED_TEST"),
            "width sensitivity gone — revisit call sites: {out:?}"
        );
    }
}
