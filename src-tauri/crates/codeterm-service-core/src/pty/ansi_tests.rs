use super::ansi::clean_terminal_output_sized;

const RECEIPTS: [&str; 3] = ["B3R:id=fixture", "B3R:bytes=16227", "B3R:sha=abcdef"];

fn conpty_repaint_fixture() -> (String, String) {
    let echo = "x".repeat(16_227);
    // ConPTY reserves the bottom three rows before repainting output at absolute addresses.
    let mut raw = format!("{echo}\r\n\r\n\r\n");
    for (row, receipt) in (41..=43).zip(RECEIPTS) {
        raw.push_str(&format!("\x1b[{row};1H\x1b[2K{receipt}"));
    }
    raw.push_str("\r\n\x1b[43;1H\x1b[2K$ ");
    (raw, format!("{echo}\n{}\n$ ", RECEIPTS.join("\n")))
}

#[test]
fn conpty_repaint_preserves_echo_and_receipts_in_order() {
    let (raw, expected) = conpty_repaint_fixture();
    assert_eq!(clean_terminal_output_sized(&raw, 43, 120), expected);
}

#[test]
fn conpty_repaint_at_incorrect_height_overlays_echo() {
    let (raw, expected) = conpty_repaint_fixture();
    let mut legacy = vt100::Parser::new(500, 120, 10_000);
    legacy.process(raw.as_bytes());
    let output = legacy.screen().contents();
    assert_ne!(output, expected);
    assert!(!RECEIPTS
        .iter()
        .all(|receipt| output.lines().any(|line| line == *receipt)));
}

#[test]
fn plain_unix_stream_preserves_lines_and_overwrites() {
    let raw = "\x1b[32mfirst\x1b[0m\r\n\r\nsecond\r\nabc\rxy";
    assert_eq!(
        clean_terminal_output_sized(raw, 24, 80),
        "first\n\nsecond\nxyc"
    );
}

#[test]
fn long_wrapped_echo_preserves_short_output() {
    let echo = "x".repeat(16_227);
    let raw = format!("{echo}\r\n{}\r\n", RECEIPTS.join("\r\n"));
    let expected = format!("{echo}\n{}", RECEIPTS.join("\n"));
    for rows in [2, 24, 43] {
        assert_eq!(clean_terminal_output_sized(&raw, rows, 120), expected);
    }
}

#[test]
fn scrollback_preserves_blank_lines_across_viewports() {
    let raw = "first\r\n\r\nsecond\r\n\r\nthird\r\nfourth";
    assert_eq!(
        clean_terminal_output_sized(raw, 2, 80),
        "first\n\nsecond\n\nthird\nfourth"
    );
}

#[test]
fn scrollback_preserves_more_than_ten_thousand_lines() {
    let expected = (0..10_050)
        .map(|n| format!("line-{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let raw = expected.replace('\n', "\r\n");
    assert_eq!(clean_terminal_output_sized(&raw, 43, 120), expected);
}

#[test]
fn zsh_partial_line_survives_real_viewport() {
    let raw = include_str!("../../../codeterm-util/testdata/zsh_partial_line_120col.raw");
    assert!(clean_terminal_output_sized(raw, 40, 120).contains("MARKER_RED_TEST"));
}

#[test]
fn undersized_viewport_clamps_to_safe_dimensions() {
    let raw = format!("{}\r\nreceipt\r\n", "x".repeat(16_227));
    let expected = clean_terminal_output_sized(&raw, 2, 120);
    assert_eq!(expected, format!("{}\nreceipt", "x".repeat(16_227)));
    for rows in [0, 1] {
        assert_eq!(clean_terminal_output_sized(&raw, rows, 120), expected);
    }
    let narrow = "abc\r\nreceipt";
    let expected = clean_terminal_output_sized(narrow, 2, 1);
    assert_eq!(expected, "abc\nreceipt");
    for rows in [0, 1, 2] {
        assert_eq!(clean_terminal_output_sized(narrow, rows, 0), expected);
    }
}
