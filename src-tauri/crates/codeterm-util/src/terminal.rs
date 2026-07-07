//! VT100 terminal emulation helpers for rendering PTY output as plain text.

pub fn clean_terminal_output(raw: &str) -> String {
    clean_terminal_output_sized(raw, 500, 200)
}

pub fn clean_terminal_output_sized(raw: &str, rows: u16, cols: u16) -> String {
    let mut parser = vt100::Parser::new(rows, cols, 10000);
    parser.process(raw.as_bytes());
    parser.screen().contents()
}
