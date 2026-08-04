use super::*;

#[test]
fn powershell_continuation_is_not_a_ready_prompt() {
    // launch_cmd would just become more input on the broken line.
    assert!(!screen_has_shell_prompt(">>"));
    assert!(!screen_has_shell_prompt(">> "));
    assert!(!screen_has_shell_prompt(">>>"));
    assert!(!screen_has_shell_prompt(">"));
    assert!(!screen_has_shell_prompt("> "));
}

#[test]
fn powershell_ready_prompt_is_recognized() {
    assert!(screen_has_shell_prompt("PS C:\\Users\\User>"));
    assert!(screen_has_shell_prompt("PS C:\\Developer\\codeterm> "));
}

#[test]
fn cmd_and_unix_prompts_are_recognized() {
    assert!(screen_has_shell_prompt("C:\\Users\\User>"));
    assert!(screen_has_shell_prompt("user@host:~$ "));
    assert!(screen_has_shell_prompt("user@host:~%"));
}

#[test]
fn claude_tui_input_cursor_is_recognized() {
    // matcher only knew POSIX/Windows shell markers.
    assert!(screen_has_shell_prompt("❯ "));
    assert!(screen_has_shell_prompt("❯"));
    // Cursor inside a box border (typical Claude TUI layout).
    assert!(screen_has_shell_prompt(
        "│ ❯                                        │"
    ));
    // Some agent TUIs use the lighter single-arrow glyph.
    assert!(screen_has_shell_prompt("› hello"));
    // Mid-action TUI screens must still report not-ready.
    assert!(!screen_has_shell_prompt("working..."));
    assert!(!screen_has_shell_prompt("Esc to interrupt"));
}

#[test]
fn bare_shell_prompt_rejects_active_tui() {
    // restart code would think the shell is back and pipe the launch command into Claude's input box.
    let claude_tui = "╭──────────────────────────╮\n│ ❯                        │";
    assert!(
        screen_has_shell_prompt(claude_tui),
        "❯ still looks shell-ish"
    );
    assert!(
        !screen_has_bare_shell_prompt(claude_tui),
        "TUI border must veto"
    );
}

#[test]
fn bare_shell_prompt_accepts_real_shell() {
    // Starship-style prompt — `❯` but no box-drawing or TUI footer.
    assert!(screen_has_bare_shell_prompt(
        "~/code/codeterm on  master\n❯ "
    ));
    assert!(screen_has_bare_shell_prompt("user@host:~$ "));
    assert!(screen_has_bare_shell_prompt("PS C:\\Users\\User> "));
}

#[test]
fn agent_tui_detected_by_universal_glyphs() {
    // Universal glyph check (box-drawing, block chars) — no text patterns here. Text patterns live in each provider's screen_has_tui()
    // implementation.
    assert!(screen_has_agent_tui("╭─ foo ─╮"));
    assert!(screen_has_agent_tui("▀▀▀▀▀▀ opencode logo"));
    assert!(!screen_has_agent_tui("some content\n? for shortcuts")); // text → provider
    assert!(!screen_has_agent_tui("working\nEsc to interrupt")); // text → provider
    assert!(!screen_has_agent_tui("just a plain shell output"));
    assert!(!screen_has_agent_tui("❯ "));
}

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
    ss.process("pty1", b"ABCDE\rXY");
    let text = ss.get_screen_text("pty1").unwrap();
    assert!(text.contains("XYCDE"));
}

#[test]
fn test_formatted_preserves_cursor_hide() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"Hello\x1b[?25l");
    let formatted = ss.get_screen_formatted("pty1").unwrap();
    let s = String::from_utf8_lossy(&formatted);
    assert!(
        s.contains("\x1b[?25l"),
        "formatted should preserve DECTCEM hide: {:?}",
        s
    );
}

#[test]
fn test_formatted_preserves_colors() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"\x1b[31mRed text\x1b[0m");
    let formatted = ss.get_screen_formatted("pty1").unwrap();
    assert!(!formatted.is_empty());
    let s = String::from_utf8_lossy(&formatted);
    assert!(s.contains("\x1b["), "formatted should contain escape codes");
    assert!(s.contains("Red text"));
}

#[test]
fn test_content_hash_changes_with_content() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"Content A");
    let hash1 = ss.get_content_hash("pty1", 0).unwrap();

    ss.process("pty1", b"\x1b[2J\x1b[HContent B");
    let hash2 = ss.get_content_hash("pty1", 0).unwrap();

    assert_ne!(hash1, hash2);
}

#[test]
fn test_content_hash_excludes_bottom_rows() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"Top line\x1b[50;1HBottom line");
    let hash_all = ss.get_content_hash("pty1", 0).unwrap();
    let hash_no_bottom = ss.get_content_hash("pty1", 3).unwrap();
    assert_ne!(hash_all, hash_no_bottom);
}

#[test]
fn cursor_input_state_clamps_a_composer_to_the_bottom_row() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"\x1b[999;1H\x1b[2m\xe2\x96\x8c\x1b[0m ");

    let (_alternate, visible, row, rows) = ss.cursor_input_state("pty1").unwrap();
    assert!(visible);
    assert!(
        row + 4 >= rows,
        "cursor row {row} must be near bottom {rows}"
    );
}

#[test]
fn resize_keeps_cursor_geometry_in_sync_with_the_pty() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"\x1b[40;1H\x1b[2m\xe2\x96\x8c\x1b[0m ");
    ss.resize("pty1", 120, 40);

    let (_alternate, visible, row, rows) = ss.cursor_input_state("pty1").unwrap();
    assert!(visible);
    assert_eq!(rows, 40);
    assert!(
        row + 4 >= rows,
        "cursor row {row} must be near bottom {rows}"
    );
}

#[test]
fn primary_screen_hidden_inverse_composer_frame_exposes_structural_caret() {
    const CAPTURED_FRAME: &str = concat!(
        "\x1b[2J",
        "\x1b[11;3HCursor Agent",
        "\x1b[18;2H\x1b[48;2;21;21;21m",
        "\x1b[2m→ \x1b[22;7mP\x1b[2;27mlan, search, build anything\x1b[22m",
        "\x1b[21;3HAuto",
        "\x1b[22;3HD:\\Developer\\codeterm · release-1.7.8",
        "\x1b[?25l\x1b[23;1H",
    );
    let mut ss = ScreenState::new();
    ss.resize("cursor", 120, 40);
    ss.process("cursor", CAPTURED_FRAME.as_bytes());

    let input = ss.composer_input_state("cursor").unwrap();
    assert!(!input.alternate_screen);
    assert!(!input.cursor_visible);
    assert_eq!(input.cursor_row, 22);
    assert_eq!(input.rows, 40);
    assert_eq!(input.cols, 120);
    assert_eq!(input.inverse_caret_row, Some(17));
    assert_eq!(input.nonblank_inverse_cells, 1);
    assert_eq!(input.sole_nonblank_inverse_row, Some(17));
    assert_eq!(input.last_nonblank_row, Some(21));
}

#[test]
fn inverse_selection_and_visible_cursor_are_not_structural_carets() {
    let mut selection = ScreenState::new();
    selection.resize("selection", 120, 23);
    selection.process(
        "selection",
        b"\x1b[?1049h\x1b[18;2H\x1b[7mAB\x1b[27m\x1b[?25l",
    );
    let selection = selection.composer_input_state("selection").unwrap();
    assert_eq!(selection.inverse_caret_row, None);
    assert_eq!(selection.nonblank_inverse_cells, 2);
    assert_eq!(selection.sole_nonblank_inverse_row, None);

    let mut visible_cursor = ScreenState::new();
    visible_cursor.resize("visible", 120, 23);
    visible_cursor.process("visible", b"\x1b[?1049h\x1b[18;2H\x1b[7mP\x1b[27m\x1b[?25h");
    let visible_cursor = visible_cursor.composer_input_state("visible").unwrap();
    assert_eq!(visible_cursor.inverse_caret_row, None);
    assert_eq!(visible_cursor.nonblank_inverse_cells, 1);
    assert_eq!(visible_cursor.sole_nonblank_inverse_row, Some(17));
}

#[test]
fn frame_without_inverse_cells_has_no_structural_caret() {
    let mut state = ScreenState::new();
    state.resize("plain", 120, 40);
    state.process("plain", b"\x1b[2J\x1b[18;2HPlan, search, build anything\x1b[?25l");

    let input = state.composer_input_state("plain").unwrap();
    assert_eq!(input.nonblank_inverse_cells, 0);
    assert_eq!(input.sole_nonblank_inverse_row, None);
    assert_eq!(input.inverse_caret_row, None);
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

#[test]
fn test_prompt_signal_fires_on_bash_prompt() {
    let mut ss = ScreenState::new();
    let sig = ss.arm_prompt_signal("pty1");
    assert!(!sig.is_ready());
    ss.process("pty1", b"user@host:~$ ");
    assert!(sig.is_ready());
}

#[test]
fn test_prompt_signal_fires_on_powershell_prompt() {
    let mut ss = ScreenState::new();
    let sig = ss.arm_prompt_signal("pty1");
    ss.process("pty1", b"PS C:\\Developer> ");
    assert!(sig.is_ready());
}

#[test]
fn test_prompt_signal_fires_immediately_if_prompt_already_present() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"user@host:~$ ");
    let sig = ss.arm_prompt_signal("pty1");
    assert!(sig.is_ready());
}

#[test]
fn test_prompt_signal_not_fired_on_plain_output() {
    let mut ss = ScreenState::new();
    let sig = ss.arm_prompt_signal("pty1");
    ss.process("pty1", b"some regular line without prompt marker");
    assert!(!sig.is_ready());
}

#[test]
fn bare_prompt_signal_rejects_agent_tui_prompt() {
    let mut ss = ScreenState::new();
    ss.process(
        "pty1",
        "╭──────────────────────────╮\n│ ❯                        │".as_bytes(),
    );
    let prompt = ss.arm_prompt_signal("pty1");
    let bare = ss.arm_bare_prompt_signal("pty1");
    assert!(
        prompt.is_ready(),
        "regular prompt signal accepts TUI composer"
    );
    assert!(
        !bare.is_ready(),
        "restore launch must not treat an agent composer as a bare shell"
    );

    ss.process("pty1", b"\x1b[2J\x1b[Huser@host:~$ ");
    assert!(
        bare.is_ready(),
        "bare prompt signal fires on real shell prompt"
    );
}

#[test]
fn bare_prompt_signal_does_not_reuse_stale_ready_state() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"user@host:~$ ");
    let first = ss.arm_bare_prompt_signal("pty1");
    assert!(first.is_ready());

    ss.process(
        "pty1",
        "\x1b[2J\x1b[H╭──────────────────────────╮\n│ ❯                        │".as_bytes(),
    );
    let second = ss.arm_bare_prompt_signal("pty1");
    assert!(
        !second.is_ready(),
        "a previous bare shell prompt must not make a later TUI screen ready"
    );
}

#[test]
fn shell_ready_accepts_an_integration_event_or_a_real_prompt() {
    let mut ss = ScreenState::new();
    let signal = ss.arm_shell_ready_signal("pty1");

    ss.process("pty1", b"ordinary output");
    assert!(!signal.is_ready());
    ss.process("pty1", b"\x1b]133;A\x07");
    assert!(signal.is_ready());
}

/// macOS bash prints Apple's zsh-migration notice before its first prompt,
/// and a login shell gets no integration script — the prompt is the only
/// readiness evidence that will ever arrive.
#[test]
fn macos_bash_zsh_migration_notice_still_reaches_shell_ready() {
    let mut ss = ScreenState::new();
    let signal = ss.arm_shell_ready_signal("pty1");

    ss.process(
        "pty1",
        "The default interactive shell is now zsh.\r\n\
         To update your account to use zsh, please run `chsh -s /bin/zsh`.\r\n\
         For more details, please visit https://support.apple.com/kb/HT208050.\r\n\
         rollacode-bot:general dev$ "
            .as_bytes(),
    );

    assert!(signal.is_ready());
}

#[test]
fn posix_and_windows_prompts_are_shell_ready_events() {
    for prompt in [
        "user@host:~$ ",
        "rollacode-bot:general dev% ",
        "PS C:\\Developer\\codeterm> ",
        "C:\\Developer>",
    ] {
        let mut ss = ScreenState::new();
        assert!(ss.bind_pty_generation("pty1", 1));

        assert!(ss.process_if_generation("pty1", 1, prompt.as_bytes()));

        assert!(
            ss.arm_shell_ready_signal("pty1").is_ready(),
            "expected readiness for {prompt:?}"
        );
    }
}

#[test]
fn continuation_prompt_does_not_mark_shell_ready() {
    let mut ss = ScreenState::new();
    assert!(ss.bind_pty_generation("pty1", 1));
    let signal = ss.arm_shell_ready_signal("pty1");

    assert!(ss.process_if_generation("pty1", 1, b">> "));

    assert!(!signal.is_ready());
}

#[test]
fn agent_composers_and_generic_output_are_not_shell_ready() {
    for output in [
        "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n\u{2502} \u{276f} \u{2502}",
        "\u{203a} ask the agent",
        "download progress >",
        "Cogitated for 4s",
    ] {
        let mut ss = ScreenState::new();
        assert!(ss.bind_pty_generation("pty1", 1));
        let signal = ss.arm_shell_ready_signal("pty1");

        assert!(ss.process_if_generation("pty1", 1, output.as_bytes()));

        assert!(!signal.is_ready(), "unexpected readiness for {output:?}");
    }
}

/// The macOS `Main` regression: the launch thread arms the readiness wait
/// before the backend reader binds the PTY generation. Binding must not tear
/// down that wait — doing so returned a false immediate "deadline elapsed"
/// and left recovery to a watchdog tick ~99s later.
#[test]
fn readiness_armed_before_the_reader_binds_survives_the_bind() {
    let mut ss = ScreenState::new();
    let signal = ss.arm_shell_ready_signal("pty1");

    assert!(ss.bind_pty_generation("pty1", 7));
    assert!(
        !signal.is_closed(),
        "first bind must not close a wait armed before the reader started"
    );

    assert!(ss.process_if_generation("pty1", 7, b"user@host:~$ "));
    assert!(signal.is_ready());
}

/// Output seen before the generation is bound belongs to the same fresh PTY,
/// so a prompt already on screen still counts once the reader binds.
#[test]
fn prompt_observed_before_bind_counts_for_the_bound_generation() {
    let mut ss = ScreenState::new();
    ss.process("pty1", b"user@host:~$ ");

    assert!(ss.bind_pty_generation("pty1", 3));

    assert!(ss.arm_shell_ready_signal("pty1").is_ready());
}

/// Replacing a generation is a real teardown — the old wait must still close.
#[test]
fn rebinding_a_new_generation_closes_the_previous_wait() {
    let mut ss = ScreenState::new();
    assert!(ss.bind_pty_generation("pty1", 1));
    let signal = ss.arm_shell_ready_signal("pty1");

    assert!(ss.bind_pty_generation("pty1", 2));

    assert!(signal.is_closed());
}

#[test]
fn shell_ready_event_before_arm_is_remembered_for_current_generation() {
    let mut ss = ScreenState::new();
    assert!(ss.bind_pty_generation("pty1", 1));

    assert!(ss.process_if_generation("pty1", 1, b"\x1b]633;A\x07"));

    assert!(ss.arm_shell_ready_signal("pty1").is_ready());
}

#[test]
fn raw_reader_generation_must_bind_before_output_is_accepted() {
    let mut ss = ScreenState::new();

    assert!(!ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
    assert!(ss.bind_pty_generation("pty1", 1));
    assert!(ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
    assert!(ss.arm_shell_ready_signal("pty1").is_ready());
}

#[test]
fn late_shell_ready_event_from_old_generation_is_rejected() {
    let mut ss = ScreenState::new();
    assert!(ss.bind_pty_generation("pty1", 1));
    assert!(ss.bind_pty_generation("pty1", 2));

    assert!(!ss.process_if_generation("pty1", 1, b"\x1b]133;A\x07"));
    let signal = ss.arm_shell_ready_signal("pty1");

    assert!(!signal.is_ready());
    ss.process("pty1", b"ordinary output");
    assert!(!signal.is_ready());
    assert!(ss.process_if_generation("pty1", 2, b"\x1b]133;A\x07"));
    assert!(signal.is_ready());
}

#[test]
fn replacing_pty_generation_cancels_armed_shell_ready_waiter() {
    let mut ss = ScreenState::new();
    assert!(ss.bind_pty_generation("pty1", 1));
    let signal = ss.arm_shell_ready_signal("pty1");

    assert!(ss.bind_pty_generation("pty1", 2));

    assert!(!signal.wait(Duration::ZERO));
}

#[test]
fn shell_event_wakes_all_waiters_and_prunes_them() {
    let mut ss = ScreenState::new();
    let first = ss.arm_bare_prompt_signal("pty1");
    let second = ss.arm_bare_prompt_signal("pty1");

    ss.process("pty1", b"user@host:~$ ");

    assert!(first.is_ready());
    assert!(second.is_ready());
    assert_eq!(ss.bare_prompt_waiter_count("pty1"), 0);
}

#[test]
fn disarmed_bare_prompt_waiter_does_not_accumulate_after_timeout() {
    let mut ss = ScreenState::new();
    let signal = ss.arm_bare_prompt_signal("pty1");

    ss.disarm_bare_prompt_signal("pty1", &signal);

    assert_eq!(ss.bare_prompt_waiter_count("pty1"), 0);
}

#[test]
fn test_prompt_signal_wait_returns_true_on_ready() {
    let mut ss = ScreenState::new();
    let sig = ss.arm_prompt_signal("pty1");
    let sig2 = sig.clone();
    let handle = std::thread::spawn(move || {
        sig2.mark_ready();
    });
    let got = sig.wait(Duration::from_secs(1));
    handle.join().unwrap();
    assert!(got);
}

#[test]
fn test_prompt_signal_wait_returns_false_on_timeout() {
    let sig = PromptSignal::new();
    let got = sig.wait(Duration::from_millis(30));
    assert!(!got);
}

#[test]
fn screen_change_signal_wakes_on_process_without_polling() {
    let mut state = ScreenState::new();
    let signal = state.arm_screen_change_signal("pty1");
    let generation = signal.generation();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = signal.clone();
    let handle = std::thread::spawn(move || {
        tx.send(waiter.wait_after(generation, Duration::from_secs(1)))
            .unwrap();
    });

    state.process("pty1", b"agent frame");

    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    handle.join().unwrap();
}

#[test]
fn unsubscribed_screen_change_signal_fails_waiters() {
    let mut state = ScreenState::new();
    let signal = state.subscribe_screen_changes("pty1");
    let generation = signal.generation();

    state.unsubscribe_screen_changes("pty1", &signal);

    assert!(!signal.wait_after(generation, Duration::ZERO));
    assert_eq!(state.screen_change_subscriber_count(), 0);
}

#[test]
fn pty_teardown_closes_all_screen_change_subscribers() {
    let mut state = ScreenState::new();
    let first = state.subscribe_screen_changes("pty1");
    let second = state.subscribe_screen_changes("pty1");
    let first_generation = first.generation();
    let second_generation = second.generation();

    state.remove("pty1");

    assert!(!first.wait_after(first_generation, Duration::ZERO));
    assert!(!second.wait_after(second_generation, Duration::ZERO));
    assert_eq!(state.screen_change_subscriber_count(), 0);
}

#[test]
fn old_generation_cannot_remove_replacement_screen() {
    let mut state = ScreenState::new();
    assert!(state.bind_pty_generation("pty1", 1));
    state.process("pty1", b"old frame");
    assert!(state.bind_pty_generation("pty1", 2));
    state.process("pty1", b"replacement frame");

    assert!(!state.remove_if_generation("pty1", 1));
    assert!(state.get_screen_text("pty1").is_some());
    assert!(state.remove_if_generation("pty1", 2));
    assert!(state.get_screen_text("pty1").is_none());
}

#[test]
fn bracketed_paste_mode_tracks_the_live_terminal_mode() {
    let mut state = ScreenState::new();
    assert_eq!(state.bracketed_paste_mode("pty1"), None);

    state.process("pty1", b"\x1b[?2004h");
    assert_eq!(state.bracketed_paste_mode("pty1"), Some(true));

    state.process("pty1", b"\x1b[?2004l");
    assert_eq!(state.bracketed_paste_mode("pty1"), Some(false));
}
