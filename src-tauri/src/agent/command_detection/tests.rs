use super::*;
use crate::agent::semantic_marker::SemanticEvent;

fn ev_pairs(det: &mut CommandDetection, evs: Vec<SemanticEvent>) -> Vec<CommandLifecycle> {
    evs.into_iter().filter_map(|e| det.apply(e)).collect()
}

#[test]
fn builds_command_list_with_exit_codes() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::Property {
                key: "Cwd".into(),
                value: "/home/me".into(),
            },
            SemanticEvent::PromptEnd,
            SemanticEvent::CommandLine {
                text: "ls -la".into(),
                nonce: None,
            },
            SemanticEvent::CommandStart,
            SemanticEvent::CommandEnd { exit_code: Some(0) },
            // Second command, non-zero exit.
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "false".into(),
                nonce: None,
            },
            SemanticEvent::CommandStart,
            SemanticEvent::CommandEnd { exit_code: Some(1) },
        ],
    );
    let cmds = det.commands();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].command_line.as_deref(), Some("ls -la"));
    assert_eq!(cmds[0].exit_code, Some(0));
    assert_eq!(cmds[0].cwd.as_deref(), Some("/home/me"));
    assert!(cmds[0].has_output_start);
    assert_eq!(cmds[1].command_line.as_deref(), Some("false"));
    assert_eq!(cmds[1].exit_code, Some(1));
}

#[test]
fn finished_lifecycle_emitted_at_command_end() {
    let mut det = CommandDetection::new();
    let out = ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandStart,
            SemanticEvent::CommandEnd { exit_code: Some(2) },
        ],
    );
    // One Started (from C) + one Finished (from D).
    assert!(matches!(out[0], CommandLifecycle::Started { .. }));
    assert!(matches!(
        out[1],
        CommandLifecycle::Finished {
            exit_code: Some(2),
            ..
        }
    ));
}

#[test]
fn nonce_mismatch_rejects_command_cycle() {
    let mut det = CommandDetection::new();
    det.ensure_nonce(Some("sekret".into()));
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "rm -rf /".into(),
                nonce: Some("forged".into()),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert!(det.commands().is_empty());
}

#[test]
fn nonce_match_accepts_command_line() {
    let mut det = CommandDetection::new();
    det.ensure_nonce(Some("sekret".into()));
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "echo hi".into(),
                nonce: Some("sekret".into()),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(det.commands()[0].command_line.as_deref(), Some("echo hi"));
}

#[test]
fn missing_nonce_rejected_when_session_nonce_set() {
    let mut det = CommandDetection::new();
    det.ensure_nonce(Some("sekret".into()));
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "echo hi".into(),
                nonce: None,
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert!(det.commands().is_empty());
}

#[test]
fn no_session_nonce_accepts_command_line() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "pwd".into(),
                nonce: Some("anything".into()),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(det.commands()[0].command_line.as_deref(), Some("pwd"));
}

#[test]
fn configured_nonce_requires_single_use_authenticated_cycle() {
    let mut det = CommandDetection::new();
    det.ensure_nonce(Some("sekret".into()));

    assert!(ev_pairs(
        &mut det,
        vec![SemanticEvent::CommandEnd { exit_code: Some(0) }],
    )
    .is_empty());
    assert!(ev_pairs(
        &mut det,
        vec![
            SemanticEvent::CommandLine {
                text: String::new(),
                nonce: Some("wrong".into()),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    )
    .is_empty());
    let authenticated = ev_pairs(
        &mut det,
        vec![
            SemanticEvent::CommandLine {
                text: String::new(),
                nonce: Some("sekret".into()),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(authenticated.len(), 1);
    assert!(matches!(
        authenticated[0],
        CommandLifecycle::Finished { .. }
    ));
    assert!(ev_pairs(
        &mut det,
        vec![SemanticEvent::CommandEnd { exit_code: Some(0) }],
    )
    .is_empty());
    assert_eq!(det.commands().len(), 1);
}

#[test]
fn cwd_deduped_prefers_property_over_osc7() {
    let mut det = CommandDetection::new();
    // OSC 7 first, then 633;P;Cwd in the same cycle — property wins, no double.
    let out = ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::Cwd {
                path: "/from/osc7".into(),
            },
            SemanticEvent::Property {
                key: "Cwd".into(),
                value: "/from/property".into(),
            },
        ],
    );
    assert_eq!(det.cwd(), Some("/from/property"));
    let changes: Vec<_> = out
        .iter()
        .filter(|e| matches!(e, CommandLifecycle::CwdChanged { .. }))
        .collect();
    // Two distinct paths => two changes, but property is the final source.
    assert_eq!(changes.len(), 2);
}

#[test]
fn cwd_property_then_osc7_does_not_clobber_within_cycle() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::Property {
                key: "Cwd".into(),
                value: "/trusted".into(),
            },
            SemanticEvent::Cwd {
                path: "/spoofed".into(),
            },
        ],
    );
    assert_eq!(det.cwd(), Some("/trusted"));
}

#[test]
fn osc7_used_as_fallback_when_no_property() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::Cwd {
                path: "/only/osc7".into(),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(det.cwd(), Some("/only/osc7"));
    assert_eq!(det.commands()[0].cwd.as_deref(), Some("/only/osc7"));
}

#[test]
fn a_d_only_sequence_yields_command_with_boundary_exit_cwd() {
    // must still produce a command with boundary, exit code, and cwd.
    let mut det = CommandDetection::new();
    let out = ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::Property {
                key: "Cwd".into(),
                value: "C:\\Users\\me".into(),
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(det.commands().len(), 1);
    let cmd = &det.commands()[0];
    assert_eq!(cmd.exit_code, Some(0));
    assert_eq!(cmd.cwd.as_deref(), Some("C:\\Users\\me"));
    assert!(!cmd.has_output_start, "no C seen in A/D-only mode");
    assert!(matches!(
        out.last(),
        Some(CommandLifecycle::Finished { .. })
    ));
}

#[test]
fn bare_d_without_prompt_still_yields_command() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![SemanticEvent::CommandEnd { exit_code: Some(7) }],
    );
    assert_eq!(det.commands().len(), 1);
    assert_eq!(det.commands()[0].exit_code, Some(7));
}

#[test]
fn decode_command_unescapes_vscode_scheme() {
    assert_eq!(decode_command("a\\x3bb"), "a;b");
    assert_eq!(decode_command("line1\\x0aline2"), "line1\nline2");
    assert_eq!(decode_command("a\\\\b"), "a\\b");
    assert_eq!(decode_command("plain text"), "plain text");
    // Uppercase hex operator accepted.
    assert_eq!(decode_command("\\x3B"), ";");
    // Non-ascii bytes pass through untouched (encoder leaves >0x20 alone).
    assert_eq!(decode_command("café"), "café");
    // Lone backslash that isn't a valid escape stays literal.
    assert_eq!(decode_command("a\\z"), "a\\z");
}

#[test]
fn command_line_text_is_decoded() {
    let mut det = CommandDetection::new();
    ev_pairs(
        &mut det,
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::CommandLine {
                text: "echo a\\x3b echo b".into(),
                nonce: None,
            },
            SemanticEvent::CommandEnd { exit_code: Some(0) },
        ],
    );
    assert_eq!(
        det.commands()[0].command_line.as_deref(),
        Some("echo a; echo b")
    );
}

#[test]
fn parser_runs_on_reader_without_disturbing_screen_state() {
    // is observed, never mutated — non-OSC text produces no events.
    let mut si = ShellIntegration::default();
    let plain = si.feed(b"just normal terminal output\r\n");
    assert!(plain.is_empty());

    // A full 633 A/E/C/D cycle split across two chunks (partial-carry).
    let mut all = Vec::new();
    all.extend(si.feed(b"\x1b]633;A\x07\x1b]633;E;ls"));
    all.extend(si.feed(b";\x07\x1b]633;C\x07\x1b]633;D;0\x07"));
    assert_eq!(si.detection().commands().len(), 1);
    assert_eq!(
        si.detection().commands()[0].command_line.as_deref(),
        Some("ls")
    );
    assert_eq!(si.detection().commands()[0].exit_code, Some(0));
}

#[test]
fn tracker_isolates_tabs_and_supplies_nonce() {
    let mut t = ShellCapabilityTracker::new();
    t.feed("pty-a", Some("nonce-a".into()), b"\x1b]633;A\x07");
    t.feed(
        "pty-a",
        Some("nonce-a".into()),
        b"\x1b]633;E;cmd-a;nonce-a\x07\x1b]633;D;0\x07",
    );
    // Wrong nonce for pty-b -> the entire command cycle is rejected.
    t.feed("pty-b", Some("nonce-b".into()), b"\x1b]633;A\x07");
    t.feed(
        "pty-b",
        Some("nonce-b".into()),
        b"\x1b]633;E;cmd-b;wrong\x07\x1b]633;D;0\x07",
    );

    let a = t.snapshot("pty-a").unwrap();
    assert_eq!(a.commands.len(), 1);
    assert_eq!(a.commands[0].command_line.as_deref(), Some("cmd-a"));

    let b = t.snapshot("pty-b").unwrap();
    assert!(b.commands.is_empty());

    t.remove("pty-a");
    assert!(t.snapshot("pty-a").is_none());
}
