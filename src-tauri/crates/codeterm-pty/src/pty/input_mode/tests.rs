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
