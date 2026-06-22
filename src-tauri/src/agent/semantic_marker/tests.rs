use super::*;

fn parse(bytes: &[u8]) -> Vec<SemanticEvent> {
    SemanticMarkerParser::new().feed(bytes)
}

// --- each marker type parses (133 + 633) ---

#[test]
fn prompt_start_a() {
    assert_eq!(parse(b"\x1b]633;A\x07"), vec![SemanticEvent::PromptStart]);
}

#[test]
fn prompt_end_b() {
    assert_eq!(parse(b"\x1b]633;B\x07"), vec![SemanticEvent::PromptEnd]);
}

#[test]
fn command_start_c() {
    assert_eq!(parse(b"\x1b]633;C\x07"), vec![SemanticEvent::CommandStart]);
}

#[test]
fn command_end_with_exit_code() {
    assert_eq!(
        parse(b"\x1b]633;D;0\x07"),
        vec![SemanticEvent::CommandEnd { exit_code: Some(0) }]
    );
    assert_eq!(
        parse(b"\x1b]633;D;127\x07"),
        vec![SemanticEvent::CommandEnd {
            exit_code: Some(127)
        }]
    );
}

#[test]
fn command_end_without_exit_code() {
    assert_eq!(
        parse(b"\x1b]633;D\x07"),
        vec![SemanticEvent::CommandEnd { exit_code: None }]
    );
}

#[test]
fn command_line_text() {
    assert_eq!(
        parse(b"\x1b]633;E;ls -la\x07"),
        vec![SemanticEvent::CommandLine {
            text: "ls -la".to_string(),
            nonce: None,
        }]
    );
}

#[test]
fn command_line_nonce_surfaced() {
    assert_eq!(
        parse(b"\x1b]633;E;git status;abc123\x07"),
        vec![SemanticEvent::CommandLine {
            text: "git status".to_string(),
            nonce: Some("abc123".to_string()),
        }]
    );
}

#[test]
fn property_cwd() {
    assert_eq!(
        parse(b"\x1b]633;P;Cwd=/home/me/proj\x07"),
        vec![SemanticEvent::Property {
            key: "Cwd".to_string(),
            value: "/home/me/proj".to_string(),
        }]
    );
}

// --- 133 compatible subset ---

#[test]
fn osc_133_subset_parses() {
    assert_eq!(parse(b"\x1b]133;A\x07"), vec![SemanticEvent::PromptStart]);
    assert_eq!(parse(b"\x1b]133;B\x07"), vec![SemanticEvent::PromptEnd]);
    assert_eq!(parse(b"\x1b]133;C\x07"), vec![SemanticEvent::CommandStart]);
    assert_eq!(
        parse(b"\x1b]133;D;2\x07"),
        vec![SemanticEvent::CommandEnd { exit_code: Some(2) }]
    );
}

// --- ST and BEL both terminate ---

#[test]
fn bel_terminator() {
    assert_eq!(parse(b"\x1b]633;A\x07"), vec![SemanticEvent::PromptStart]);
}

#[test]
fn st_terminator() {
    assert_eq!(parse(b"\x1b]633;A\x1b\\"), vec![SemanticEvent::PromptStart]);
}

#[test]
fn st_terminator_on_payload() {
    assert_eq!(
        parse(b"\x1b]633;E;echo hi\x1b\\"),
        vec![SemanticEvent::CommandLine {
            text: "echo hi".to_string(),
            nonce: None,
        }]
    );
}

// --- OSC 7 cwd ---

#[test]
fn osc7_file_url() {
    assert_eq!(
        parse(b"\x1b]7;file://host/home/me/proj\x07"),
        vec![SemanticEvent::Cwd {
            path: "/home/me/proj".to_string(),
        }]
    );
}

#[test]
fn osc7_st_terminator() {
    assert_eq!(
        parse(b"\x1b]7;file://host/var/log\x1b\\"),
        vec![SemanticEvent::Cwd {
            path: "/var/log".to_string(),
        }]
    );
}

#[test]
fn osc7_percent_decoded() {
    assert_eq!(
        parse(b"\x1b]7;file://host/home/my%20dir\x07"),
        vec![SemanticEvent::Cwd {
            path: "/home/my dir".to_string(),
        }]
    );
}

#[test]
fn osc7_windows_drive_strips_leading_slash() {
    assert_eq!(
        parse(b"\x1b]7;file:///C:/Users/me\x07"),
        vec![SemanticEvent::Cwd {
            path: "C:/Users/me".to_string(),
        }]
    );
}

// OSC 9;9 (ConEmu / Windows-friendly cwd) — raw path, no nonce.
#[test]
fn osc9_windows_friendly_cwd() {
    assert_eq!(
        parse(b"\x1b]9;9;/home/me/proj\x07"),
        vec![SemanticEvent::Cwd {
            path: "/home/me/proj".to_string(),
        }]
    );
}

#[test]
fn osc9_non_cwd_subcommand_ignored() {
    assert_eq!(parse(b"\x1b]9;4;3;progress\x07"), Vec::new());
}

// OSC 1337;CurrentDir= (iTerm2) — raw path, no nonce.
#[test]
fn osc1337_current_dir() {
    assert_eq!(
        parse(b"\x1b]1337;CurrentDir=/home/me\x07"),
        vec![SemanticEvent::Cwd {
            path: "/home/me".to_string(),
        }]
    );
}

#[test]
fn osc1337_non_cwd_key_ignored() {
    assert_eq!(parse(b"\x1b]1337;RemoteHost=user@host\x07"), Vec::new());
}

// Boundary: sub-ids outside this parser's typed-event set produce nothing
// (RightPrompt H, EnvJson, etc. are OSC3 capability concerns, not ours).
#[test]
fn right_prompt_ignored() {
    assert_eq!(parse(b"\x1b]633;H\x07"), Vec::new());
}

#[test]
fn env_json_ignored() {
    assert_eq!(parse(b"\x1b]633;EnvJson;{}\x07;nonce\x07"), Vec::new());
}

// --- streaming: split across two chunks ---

#[test]
fn split_across_two_chunks() {
    let mut p = SemanticMarkerParser::new();
    assert_eq!(p.feed(b"\x1b]633;E;ls -"), Vec::new());
    assert_eq!(
        p.feed(b"la\x07"),
        vec![SemanticEvent::CommandLine {
            text: "ls -la".to_string(),
            nonce: None,
        }]
    );
}

#[test]
fn split_mid_introducer() {
    let mut p = SemanticMarkerParser::new();
    assert_eq!(p.feed(b"output\x1b"), Vec::new());
    assert_eq!(p.feed(b"]633;A\x07"), vec![SemanticEvent::PromptStart]);
}

#[test]
fn split_st_terminator_across_chunks() {
    let mut p = SemanticMarkerParser::new();
    assert_eq!(p.feed(b"\x1b]633;C\x1b"), Vec::new());
    assert_eq!(p.feed(b"\\"), vec![SemanticEvent::CommandStart]);
}

// --- robustness: malformed / partial neither panic nor emit ---

#[test]
fn non_osc_bytes_pass_through_untouched() {
    assert_eq!(parse(b"just some normal output\n"), Vec::new());
}

#[test]
fn embedded_markers_among_normal_output() {
    assert_eq!(
        parse(b"before\x1b]633;A\x07middle\x1b]633;B\x07after"),
        vec![SemanticEvent::PromptStart, SemanticEvent::PromptEnd]
    );
}

#[test]
fn unknown_osc_code_ignored() {
    assert_eq!(parse(b"\x1b]0;window title\x07"), Vec::new());
}

#[test]
fn unknown_133_subcommand_ignored() {
    assert_eq!(parse(b"\x1b]633;Z;whatever\x07"), Vec::new());
}

#[test]
fn malformed_exit_code_yields_none() {
    assert_eq!(
        parse(b"\x1b]633;D;notanumber\x07"),
        vec![SemanticEvent::CommandEnd { exit_code: None }]
    );
}

#[test]
fn unterminated_sequence_does_not_emit() {
    let mut p = SemanticMarkerParser::new();
    assert_eq!(p.feed(b"\x1b]633;A"), Vec::new());
}

#[test]
fn empty_chunk_is_safe() {
    let mut p = SemanticMarkerParser::new();
    assert_eq!(p.feed(b""), Vec::new());
}

#[test]
fn lone_esc_is_safe() {
    assert_eq!(parse(b"\x1b"), Vec::new());
}

#[test]
fn multiple_markers_one_chunk() {
    assert_eq!(
        parse(b"\x1b]633;A\x07\x1b]633;B\x07\x1b]633;C\x07"),
        vec![
            SemanticEvent::PromptStart,
            SemanticEvent::PromptEnd,
            SemanticEvent::CommandStart,
        ]
    );
}
