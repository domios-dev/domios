//! WHAT: paste-frame encoding at the transport seam.
//! WHY: a chat line must arrive as one frame, a bootstrap as one hidden turn.
//! ORACLE: the encoded frames themselves, independent of any submit harness.
use super::tests::tui;
use super::*;

#[test]
fn xterm_paste_normalizes_newlines_and_obeys_the_live_mode() {
    let text = "alpha\r\nbeta\ngamma\rdelta";
    // Inside bracketed paste, \r is the provider's own Enter byte and splits one
    // message into several turns — only the CRLF pair is folded, not a standalone \r.
    assert_eq!(
        xterm_paste(text, Some(true), PasteEncodingCapability::LiveModeOnly),
        "\x1b[200~alpha\nbeta\ngamma\rdelta\x1b[201~"
    );
    assert_eq!(
        xterm_paste(text, Some(false), PasteEncodingCapability::MultilineTui),
        "alpha\rbeta\rgamma\rdelta"
    );
    for strategy in [
        VerifySubmitStrategy::ShellPrompt {
            timeout: Duration::ZERO,
        },
        VerifySubmitStrategy::None,
    ] {
        assert_eq!(
            xterm_paste(text, None, PasteEncodingCapability::for_strategy(&strategy)),
            "alpha\rbeta\rgamma\rdelta"
        );
    }
}

#[test]
fn xterm_paste_keeps_single_line_chat_input_in_one_paste_frame() {
    assert_eq!(
        xterm_paste("payload", Some(true), PasteEncodingCapability::LiveModeOnly),
        "\x1b[200~payload\x1b[201~"
    );
}

#[test]
fn routed_tags_are_typed_around_a_bracketed_body_only() {
    let open = "<domios from=\"tab\" label=\"sender\" tab=\"abc12345\">";
    let body = "line one\nline two";
    let suffix = format!(
        " [reply hint] {}{}",
        codeterm_types::domios::DOMIOS_TYPED_INSTRUCTION,
        codeterm_types::domios::DOMIOS_CLOSE_TAG
    );
    let encoded = encode_paste_frame(
        body,
        Some(open),
        Some(&suffix),
        Some(false),
        PasteEncodingCapability::LiveModeOnly,
    );
    let encoded = String::from_utf8(encoded).unwrap();

    assert_eq!(
        encoded,
        format!("{open}{BRACKETED_PASTE_START}line one\nline two{BRACKETED_PASTE_END}{suffix}")
    );
}

#[derive(Default)]
struct SubmitSemantics {
    payload_writes: usize,
    bracketed_insertions: usize,
    submit_boundaries: usize,
    provider_rows: Vec<String>,
}

fn model_submit_semantics(frames: &[Vec<u8>]) -> SubmitSemantics {
    let mut semantics = SubmitSemantics::default();
    let mut composer = String::new();
    for frame in frames {
        let frame = std::str::from_utf8(frame).expect("oracle frames are UTF-8");
        if frame != "\r" {
            semantics.payload_writes += 1;
        }
        if let Some(content) = frame
            .strip_prefix(BRACKETED_PASTE_START)
            .and_then(|content| content.strip_suffix(BRACKETED_PASTE_END))
        {
            semantics.bracketed_insertions += 1;
            composer.push_str(content);
            continue;
        }
        for ch in frame.chars() {
            if ch == '\r' {
                semantics.submit_boundaries += 1;
                semantics.provider_rows.push(std::mem::take(&mut composer));
            } else {
                composer.push(ch);
            }
        }
    }
    semantics
}

#[test]
fn multiline_tui_bootstrap_encodes_as_one_hidden_system_turn() {
    let opaque_body = "TOKEN_A7\nTOKEN_B9\nTOKEN_C3";
    let payload = crate::service::messenger::Messenger::from(
        crate::service::messenger::MessageEnvelope::codeterm_reminder("general-bootstrap"),
    )
    .wrap(opaque_body);
    assert_eq!(payload.matches(crate::message_markers::OPEN).count(), 0);
    assert!(payload.starts_with("<domios from=\"system\""));

    let frames = initial_submit_frames(
        &payload,
        b"\r",
        None,
        PasteEncodingCapability::for_strategy(&tui()),
    );
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1], b"\r");

    let semantics = model_submit_semantics(&frames);
    assert_eq!(semantics.payload_writes, 1);
    assert_eq!(semantics.bracketed_insertions, 1);
    assert_eq!(semantics.submit_boundaries, 1);
    assert_eq!(semantics.provider_rows.len(), 1);

    let mut hidden_system = Vec::new();
    let mut ordinary_users = Vec::new();
    for row in &semantics.provider_rows {
        match crate::message_markers::parse_host_injected(row) {
            Some(marker) if marker.kind == "system" => hidden_system.push(marker),
            _ => ordinary_users.push(row),
        }
    }
    assert_eq!(hidden_system.len(), 1);
    assert!(ordinary_users.is_empty());
    let hidden_system = &hidden_system[0];
    assert_eq!(hidden_system.kind, "system");
    assert_eq!(hidden_system.attr("context"), Some("general-bootstrap"));
    assert_eq!(hidden_system.payload.replace('\r', "\n"), opaque_body);
}

#[test]
fn prompt_line_launch_brief_uses_one_frame_even_when_live_paste_mode_is_disabled() {
    let brief = "opaque guidance\nopaque executor\nopaque assignment";
    let prefix = crate::service::launch_delivery::launch_input_frame_prefix(
        crate::agent::provider::ComposerCursorPosition::PromptLine,
    );
    let encoded = encode_paste_frame(
        brief,
        prefix,
        None,
        Some(false),
        PasteEncodingCapability::MultilineTui,
    );
    assert_eq!(
        encoded,
        format!("{BRACKETED_PASTE_START}{brief}{BRACKETED_PASTE_END}").as_bytes()
    );
    for composer in [
        crate::agent::provider::ComposerCursorPosition::BottomRegion,
        crate::agent::provider::ComposerCursorPosition::VisibleAnywhere,
    ] {
        assert!(crate::service::launch_delivery::launch_input_frame_prefix(composer).is_none());
    }
}

#[test]
fn declared_paste_refusal_overrides_live_mode_multiline_and_typed_frames() {
    let declaration = serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": false
    }))
    .unwrap();
    for strategy in [tui(), VerifySubmitStrategy::None] {
        let capability = PasteEncodingCapability::for_provider(&strategy, Some(&declaration));
        for live_mode in [None, Some(false), Some(true)] {
            for (text, expected) in [
                ("opaque", "opaque"),
                ("a\r\nb\nc", "a\rb\rc"),
                ("{opaque\none\ntwo\nopaque}", "{opaque\rone\rtwo\ropaque}"),
            ] {
                for (prefix, suffix) in [
                    (None, None),
                    (Some("<opaque>"), None),
                    (None, Some("</opaque>")),
                    (Some("<opaque>"), Some("</opaque>")),
                ] {
                    assert_eq!(
                        encode_paste_frame(text, prefix, suffix, live_mode, capability),
                        format!(
                            "{}{expected}{}",
                            prefix.unwrap_or_default(),
                            suffix.unwrap_or_default()
                        )
                        .as_bytes()
                    );
                }
            }
        }
    }
}

#[test]
fn absent_or_supported_paste_declarations_preserve_tui_framing() {
    let supported = serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": true
    }))
    .unwrap();
    for declaration in [None, Some(&supported)] {
        let capability = PasteEncodingCapability::for_provider(&tui(), declaration);
        assert_eq!(
            encode_paste_frame("a\nb", Some("<opaque>"), None, Some(false), capability),
            b"<opaque>\x1b[200~a\nb\x1b[201~"
        );
    }
}
