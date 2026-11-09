use super::*;

#[test]
fn win32_ctrl_keys_preserve_virtual_key_and_ascii_base_character() {
    for letter in b'a'..=b'z' {
        let vk = letter.to_ascii_uppercase();
        let unicode = letter;
        for prefix in ["ctrl-", "control-", "c-"] {
            let key = format!("{prefix}{}", char::from(letter));
            assert_eq!(
                encode_action_key(&key, true, true).unwrap(),
                format!("\x1b[{vk};0;{unicode};1;8;1_\x1b[{vk};0;{unicode};0;8;1_").into_bytes()
            );
        }
    }
    assert_eq!(
        encode_action_key("CTRL_C", true, true).unwrap(),
        b"\x1b[67;0;99;1;8;1_\x1b[67;0;99;0;8;1_"
    );
    assert_eq!(
        encode_action_key("ctrl-[", true, true).unwrap(),
        b"\x1b[219;0;91;1;8;1_\x1b[219;0;91;0;8;1_"
    );
}

#[test]
fn win32_navigation_and_escape_have_explicit_virtual_keys() {
    for (key, vk, unicode, modifiers) in [
        ("esc", 27, 27, 0),
        ("up", 38, 0, 256),
        ("down", 40, 0, 256),
        ("left", 37, 0, 256),
        ("right", 39, 0, 256),
        ("home", 36, 0, 256),
        ("end", 35, 0, 256),
        ("pageup", 33, 0, 256),
        ("pagedown", 34, 0, 256),
        ("delete", 46, 0, 256),
        ("backspace", 8, 8, 0),
        ("tab", 9, 9, 0),
        ("enter", 13, 13, 0),
    ] {
        assert_eq!(
            encode_action_key(key, true, true).unwrap(),
            format!("\x1b[{vk};0;{unicode};1;{modifiers};1_\x1b[{vk};0;{unicode};0;{modifiers};1_")
                .into_bytes()
        );
    }
    assert_ne!(
        encode_action_key("ctrl-i", true, true),
        encode_action_key("tab", true, true)
    );
    assert_ne!(
        encode_action_key("ctrl-m", true, true),
        encode_action_key("enter", true, true)
    );
}

#[test]
fn unix_and_unnegotiated_windows_keep_original_bytes() {
    for key in [
        "ctrl-c", "ctrl-i", "ctrl-m", "esc", "up", "delete", "A", "с", "✓", "space", "unknown",
    ] {
        for (windows, win32) in [(false, false), (false, true), (true, false)] {
            assert_eq!(
                encode_action_key(key, windows, win32),
                action_key_bytes(key)
            );
        }
    }
    for key in ["A", "с", "✓", "space"] {
        assert_eq!(encode_action_key(key, true, true), action_key_bytes(key));
    }
    assert_eq!(encode_action_key("unknown", true, true), None);
}

#[test]
fn negotiation_survives_every_chunk_boundary_and_honors_reset() {
    let request = b"\x1b[?1;9001h";
    for boundary in 0..=request.len() {
        let mut mode = InputMode::default();
        mode.process(&request[..boundary]);
        mode.process(&request[boundary..]);
        assert!(mode.win32());
        mode.process(b"\x1b[?9001l");
        assert!(!mode.win32());
    }
}

#[test]
fn negotiation_ignores_other_modes_and_string_payloads() {
    let mut mode = InputMode::default();
    mode.process(b"\x1b[9001h\x1b[?2004h\x1b]0;9001h\x07\x1bP9001h\x1b\\");
    assert!(!mode.win32());
    mode.process(b"\x1b[?9001h");
    mode.process(b"\x1b[?2004l");
    assert!(mode.win32());
}

#[test]
fn screen_mode_is_per_pty_and_generation() {
    let screens = crate::pty::screen_state::ScreenState::new();
    screens.bind_pty_generation("one", 1);
    assert!(screens.process_if_generation("one", 1, b"\x1b[?9001h"));
    assert!(screens.win32_input_mode("one"));
    assert!(!screens.win32_input_mode("two"));
    screens.bind_pty_generation("one", 2);
    assert!(!screens.win32_input_mode("one"));
    assert!(!screens.process_if_generation("one", 1, b"\x1b[?9001h"));
    assert!(!screens.win32_input_mode("one"));
}

#[test]
fn application_modes_require_negotiation_and_reset_at_each_shell_prompt_boundary() {
    let enable = b"\x1b[?2004;1003;1006;1004;9001h";
    let reports = b"\x1b[<0;4;5M\x1b[<35;4;5M\x1b[I\x1b[O";
    for prompt in [b"\x1b]633;A\x07".as_slice(), b"\x1b]133;A\x1b\\"] {
        for boundary in 0..=prompt.len() {
            let mut mode = InputMode::default();
            assert_eq!(mode.filter_terminal_reports(reports), b"");
            for byte in enable {
                mode.process(&[*byte]);
            }
            assert!(mode.bracketed_paste());
            assert_eq!(mode.filter_terminal_reports(reports), reports);
            mode.process(&prompt[..boundary]);
            mode.process(&prompt[boundary..]);
            assert!(!mode.bracketed_paste());
            assert!(mode.win32());
            assert_eq!(mode.filter_terminal_reports(reports), b"");
            mode.process(enable);
            assert_eq!(mode.filter_terminal_reports(reports), reports);
            mode.process(b"\x1b[?2004;1003;1006;1004l");
            assert!(!mode.bracketed_paste());
            assert_eq!(mode.filter_terminal_reports(reports), b"");
        }
    }
}

#[test]
fn report_protocols_and_mouse_motion_require_the_matching_modes() {
    let mut mode = InputMode::default();
    let legacy = b"\x1b[M !!";
    let sgr = b"\x1b[<0;1;1M";
    let urxvt = b"\x1b[32;1;1M";
    mode.process(b"\x1b[?1006h");
    assert_eq!(mode.filter_terminal_reports(sgr), b"");
    mode.process(b"\x1b[?1000h");
    assert_eq!(mode.filter_terminal_reports(sgr), sgr);
    assert_eq!(mode.filter_terminal_reports(legacy), b"");
    assert_eq!(mode.filter_terminal_reports(urxvt), b"");
    assert_eq!(mode.filter_terminal_reports(b"\x1b[<32;1;1M"), b"");
    mode.process(b"\x1b[?1002h");
    assert_eq!(
        mode.filter_terminal_reports(b"\x1b[<32;1;1M"),
        b"\x1b[<32;1;1M"
    );
    assert_eq!(mode.filter_terminal_reports(b"\x1b[<35;1;1M"), b"");
    mode.process(b"\x1b[?1003h");
    assert_eq!(
        mode.filter_terminal_reports(b"\x1b[<35;1;1M"),
        b"\x1b[<35;1;1M"
    );
    mode.process(b"\x1b[?1006l");
    assert_eq!(mode.filter_terminal_reports(sgr), b"");
    assert_eq!(mode.filter_terminal_reports(legacy), legacy);
    mode.process(b"\x1b[?1015h");
    assert_eq!(mode.filter_terminal_reports(urxvt), urxvt);
    assert_eq!(mode.filter_terminal_reports(legacy), b"");
    assert_eq!(mode.filter_terminal_reports(b"\x1b[I"), b"");
    mode.process(b"\x1b[?1004h");
    assert_eq!(mode.filter_terminal_reports(b"\x1b[I"), b"\x1b[I");
}

#[test]
fn terminal_resets_and_generation_replacement_discard_application_modes() {
    for reset in [b"\x1bc".as_slice(), b"\x1b[!p"] {
        let mut mode = InputMode::default();
        mode.process(b"\x1b[?2004;1003;1006;1004h");
        mode.process(reset);
        assert!(!mode.bracketed_paste());
        assert_eq!(mode.filter_terminal_reports(b"\x1b[<0;1;1m\x1b[I"), b"");
    }
    let screens = crate::pty::screen_state::ScreenState::new();
    screens.bind_pty_generation("reports", 1);
    screens.process_if_generation("reports", 1, b"\x1b[?2004;1003;1006;1004h");
    screens.bind_pty_generation("reports", 2);
    assert!(!screens.process_if_generation("reports", 1, b"\x1b[?2004;1003;1006;1004h"));
    assert_eq!(
        screens.filter_terminal_reports("reports", b"\x1b[<0;1;1m\x1b[I"),
        b""
    );
    assert_eq!(
        screens.filter_terminal_reports("unknown", b"\x1b[Ikey\x1b[O"),
        b"key"
    );
}

#[test]
fn report_filter_preserves_keyboard_bytes_and_complete_paste_contents() {
    let mut mode = InputMode::default();
    let keys = "echo λ\r\x1b[A\x1b\x03".as_bytes();
    assert_eq!(mode.filter_terminal_reports(keys), keys);
    assert_eq!(
        mode.filter_terminal_reports(b"a\x1b[<0;3;4mb\x1b[Ic\x1b[Od"),
        b"abcd"
    );
    let paste = b"\x1b[200~opaque\x1b[Itext\x1b[201~";
    assert_eq!(mode.filter_terminal_reports(paste), b"opaque\x1b[Itext");
    mode.process(b"\x1b[?2004h");
    assert_eq!(mode.filter_terminal_reports(paste), paste);
}
