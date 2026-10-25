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
        xterm_paste(text, Some(false), PasteEncodingCapability::LiveModeOnly),
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
fn complete_routed_frame_is_one_bracketed_insertion_with_both_tags() {
    for (body, suffix) in [
        ("opaque body\nopaque continuation", " [opaque hint]"),
        (
            "opaque first line\nopaque second line",
            "\n<tip>opaque suffix</tip>",
        ),
    ] {
        let frame = codeterm_wire::domios::DomiosFrame {
            opening_tag: codeterm_wire::domios::build_opening_tag(&[
                ("from", "tab"),
                ("tab", "abc12345"),
            ]),
            body: body.into(),
            suffix: suffix.into(),
        };
        let message = frame.to_message();
        for live_mode in [Some(true), Some(false)] {
            let encoded = encode_paste_payload(
                &message,
                false,
                None,
                live_mode,
                PasteEncodingCapability::MultilineTui,
            );
            assert_eq!(
                encoded,
                format!("{BRACKETED_PASTE_START}{message}{BRACKETED_PASTE_END}").as_bytes()
            );
            let semantics = model_submit_semantics(&[encoded, b"\r".to_vec()]);
            assert_eq!(semantics.payload_writes, 1);
            assert_eq!(semantics.bracketed_insertions, 1);
            assert_eq!(semantics.submit_boundaries, 1);
            assert_eq!(semantics.provider_rows, vec![message.clone()]);
        }
    }
}

#[test]
fn nonrouted_marker_shares_the_body_paste_only_when_bracketed() {
    let marker = "\n::tail::";
    assert_eq!(
        encode_paste_payload(
            "one\ntwo",
            false,
            Some(marker),
            Some(true),
            PasteEncodingCapability::MultilineTui
        ),
        b"\x1b[200~one\ntwo\n::tail::\x1b[201~"
    );
    for (mode, capability) in [
        (Some(false), PasteEncodingCapability::LiveModeOnly),
        (Some(true), PasteEncodingCapability::Unsupported),
        (Some(true), PasteEncodingCapability::TaggedBlock),
    ] {
        let bytes = encode_paste_payload("one\ntwo", false, Some(marker), mode, capability);
        assert!(!String::from_utf8(bytes).unwrap().contains("::tail::"));
    }
}

#[test]
fn large_38_line_paste_keeps_body_and_marker_in_one_provider_event() {
    // DEV's failed input had 4984 bytes and 38 lines; the second paste committed only its marker.
    let text = format!(
        "{}\n\n{}\n\n{}",
        "a".repeat(171),
        "b".repeat(15),
        format!("{}\n", "c".repeat(140)).repeat(34)
    );
    assert_eq!(text.len(), 4984);
    assert_eq!(text.lines().count(), 38);
    let marker = codeterm_wire::domios::pasted_marker_tail();
    assert_eq!(marker.len(), 17);
    let expected = format!("{text}{marker}");
    for mode in [Some(true), Some(false), None] {
        let encoded = encode_paste_payload(
            &text,
            false,
            Some(&marker),
            mode,
            PasteEncodingCapability::MultilineTui,
        );
        let wire = std::str::from_utf8(&encoded).unwrap();
        assert_eq!(wire.matches(BRACKETED_PASTE_START).count(), 1);
        assert_eq!(wire.matches(BRACKETED_PASTE_END).count(), 1);
        assert_eq!(encoded.len(), expected.len() + 12);
        assert_eq!(
            wire.strip_prefix(BRACKETED_PASTE_START)
                .unwrap()
                .strip_suffix(BRACKETED_PASTE_END)
                .unwrap(),
            expected
        );
        let semantics = model_submit_semantics(&[encoded, b"\r".to_vec()]);
        assert_eq!(semantics.payload_writes, 1);
        assert_eq!(semantics.bracketed_insertions, 1);
        assert_eq!(semantics.submit_boundaries, 1);
        assert_eq!(semantics.provider_rows, vec![expected.clone()]);
    }
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
fn prompt_line_launch_brief_forces_one_frame_when_live_mode_is_disabled() {
    let brief = "opaque guidance";
    assert_eq!(
        encode_paste_payload(
            brief,
            true,
            None,
            Some(false),
            PasteEncodingCapability::MultilineTui
        ),
        b"\x1b[200~opaque guidance\x1b[201~"
    );
}

#[test]
fn declared_paste_refusal_overrides_live_mode_multiline_and_launch_force() {
    let declaration =
        serde_json::from_value(serde_json::json!({"supportsBracketedPaste": false})).unwrap();
    for strategy in [tui(), VerifySubmitStrategy::None] {
        let capability = PasteEncodingCapability::for_provider(&strategy, Some(&declaration));
        for mode in [None, Some(false), Some(true)] {
            for force in [false, true] {
                assert_eq!(
                    encode_paste_payload("a\r\nb\nc", force, None, mode, capability),
                    b"a\rb\rc"
                );
            }
        }
    }
}

#[test]
fn absent_or_supported_paste_declarations_preserve_tui_framing() {
    let supported =
        serde_json::from_value(serde_json::json!({"supportsBracketedPaste": true})).unwrap();
    for declaration in [None, Some(&supported)] {
        let capability = PasteEncodingCapability::for_provider(&tui(), declaration);
        assert_eq!(
            encode_paste_payload("a\nb", false, None, Some(false), capability),
            b"\x1b[200~a\nb\x1b[201~"
        );
    }
}

fn tagged_declaration() -> crate::agent::provider::PasteCapability {
    serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": false, "multiline": "tagged_block"
    }))
    .unwrap()
}

fn tagged_plain_reader(frames: &[Vec<u8>]) -> Vec<String> {
    let mut turns = Vec::new();
    let mut tag = None;
    let mut body = Vec::new();
    let mut line = String::new();
    let input = frames.concat();
    for character in std::str::from_utf8(&input).unwrap().chars() {
        if character != '\r' {
            line.push(character);
            continue;
        }
        let current = std::mem::take(&mut line);
        if let Some(closing_tag) = &tag {
            if current == format!("{closing_tag}}}") {
                turns.push(body.join("\n"));
                body.clear();
                tag = None;
            } else {
                body.push(current);
            }
        } else if let Some(open) = current.strip_prefix('{') {
            tag = Some(open.to_string());
        } else {
            turns.push(current);
        }
    }
    assert!(line.is_empty());
    assert!(tag.is_none());
    turns
}

#[test]
fn refused_bracketed_multiline_uses_one_declared_submission_for_every_frame_shape() {
    let declaration = tagged_declaration();
    let capability = PasteEncodingCapability::for_provider(&tui(), Some(&declaration));
    for live_mode in [None, Some(false), Some(true)] {
        for body in [
            "one\ntwo\nthree",
            "one\r\ntwo\nthree\n",
            "one\rtwo",
            "one\ndomios}\ndomios1}",
            "one\nλ😀",
        ] {
            for (prefix, suffix) in [
                (None, None),
                (Some("<route>"), Some("</route>")),
                (Some("<route>\n"), Some("\n</route>")),
                (Some(""), None),
            ] {
                let payload = format!(
                    "{}{}{}",
                    prefix.unwrap_or_default(),
                    body,
                    suffix.unwrap_or_default()
                );
                let frame = encode_paste_payload(&payload, false, None, live_mode, capability);
                assert!(!frame.contains(&0x1b));
                let expected = format!(
                    "{}{}{}",
                    prefix.unwrap_or_default(),
                    body,
                    suffix.unwrap_or_default()
                )
                .replace("\r\n", "\n")
                .replace('\r', "\n");
                let frames = vec![frame, b"\r".to_vec()];
                assert_eq!(tagged_plain_reader(&frames), vec![expected]);
            }
        }
        assert_eq!(
            encode_paste_payload("opaque", false, None, live_mode, capability),
            b"opaque"
        );
    }
}

#[test]
fn cli_pty_send_resolves_multiline_declaration_without_chat_preprocessors() {
    use crate::agent::registry::AgentRegistry;
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_plugin::js_runtime::{JsProvider, PluginVm};

    let raw = serde_json::json!({
        "id": "plain-reader", "version": "0.1.0", "displayName": "Plain Reader",
        "spawn": {
            "paste": {"supportsBracketedPaste": false, "multiline": "tagged_block"},
            "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
            "taskDelivery": "pty_input", "verifySubmit": {"kind": "none"}
        }
    })
    .to_string();
    let manifest = serde_json::from_str(&raw).unwrap();
    let vm = PluginVm::from_source_with(
        "plain-reader",
        "module.exports.default = {};".into(),
        Some(raw),
    )
    .unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
    let (mut mux, tab) = MultiplexerState::new();
    mux.link_pty(&tab, "cli-target-pty".into());
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("plain-reader".into());
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let sibling = mux.create_tab(&workspace).unwrap();
    mux.link_pty(&sibling, "sibling-pty".into());
    mux.get_tab_mut(&sibling).unwrap().agent_type = Some("other-reader".into());
    let mux = Arc::new(codeterm_util::lock_hold::TimedRwLock::new("test-mux", mux));
    let (provider, capability) = paste_encoding::resolve_pty_paste_capability(
        &mux,
        &registry,
        "cli-target-pty",
        &VerifySubmitStrategy::None,
    );
    assert_eq!(provider.as_deref(), Some("plain-reader"));
    let payload = "one\ntwo\nthree\n";
    let frames = initial_submit_frames(payload, b"\r", Some(true), capability);
    assert_eq!(tagged_plain_reader(&frames), vec![payload]);
    let (_, sibling_capability) = paste_encoding::resolve_pty_paste_capability(
        &mux,
        &registry,
        "sibling-pty",
        &VerifySubmitStrategy::None,
    );
    assert_eq!(sibling_capability, PasteEncodingCapability::LiveModeOnly);
}

#[test]
fn long_routed_report_and_next_sender_each_have_one_complete_paste() {
    let body: String = (0..1600).map(|n| format!("{n:04x}λ8f1c9a\n")).collect();
    assert!(body.len() >= 20_000);
    let capability = serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": true,
        "marksRoutedMessages": true
    }))
    .unwrap();
    let frames = [
        crate::service::messenger::Messenger::report("opaque-a", "a813b7e2").routed_frame(&body),
        crate::service::messenger::Messenger::message("opaque-b", "b925d6f3")
            .routed_frame("72d4f9a1\n90c3e8b5"),
    ];
    for frame in frames {
        let input = crate::service::pty_io::send::prepare_routed_input(&frame, Some(&capability));
        let encoded = encode_paste_payload(
            &input.body,
            false,
            None,
            Some(false),
            PasteEncodingCapability::MultilineTui,
        );
        let expected = format!(
            "{}{}{}{}{}",
            frame.opening_tag,
            frame.body,
            codeterm_wire::domios::pasted_marker_tail(),
            frame.suffix,
            codeterm_wire::domios::DOMIOS_CLOSE_TAG,
        );
        assert_eq!(encoded, format!("\x1b[200~{expected}\x1b[201~").as_bytes());
        let parsed = codeterm_wire::domios::parse_message(&input.body).unwrap();
        assert_eq!(parsed.body, frame.body);
        assert_eq!(
            parsed.content,
            format!(
                "{}{}{}",
                frame.body,
                codeterm_wire::domios::pasted_marker_tail(),
                frame.suffix
            )
        );
    }
}

#[test]
fn every_non_human_agent_delivery_has_exactly_one_frame_in_one_paste() {
    use crate::service::messenger::{MessageEnvelope, MessageKind, Messenger};

    let capability = serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": true,
        "marksRoutedMessages": true
    }))
    .unwrap();
    let body = "opaque alpha\nopaque beta";
    let mut cases = vec![
        (MessageEnvelope::tab("Sender", "source"), "tab"),
        (MessageEnvelope::tab("Self", "target"), "tab"),
        (MessageEnvelope::default(), "cli"),
        (MessageEnvelope::report("Worker", "source"), "report"),
        (MessageEnvelope::report("", ""), "report"),
        (MessageEnvelope::from_codeterm(), "system"),
        (MessageEnvelope::codeterm_reminder("build"), "system"),
        (
            MessageEnvelope::codeterm_reminder("curator:round_started:7"),
            "system",
        ),
        (MessageEnvelope::codeterm_reminder("curator"), "system"),
        (MessageEnvelope::reminder("Sender", "source"), "reminder"),
        (MessageEnvelope::pebble(), "pebble"),
    ];
    for (kind, from) in [
        (MessageKind::Mesh, "mesh"),
        (MessageKind::Telegram, "telegram"),
    ] {
        cases.push((
            MessageEnvelope {
                kind,
                ..Default::default()
            },
            from,
        ));
    }
    for (envelope, from) in cases {
        let frame = Messenger::from(envelope.clone())
            .delivery_frame(true, body)
            .unwrap();
        let input = crate::service::pty_io::send::prepare_routed_input(
            &frame.to_frame(),
            Some(&capability),
        );
        let bytes = encode_paste_payload(
            &input.body,
            false,
            None,
            Some(false),
            PasteEncodingCapability::MultilineTui,
        );
        let encoded = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(encoded.matches(BRACKETED_PASTE_START).count(), 1);
        assert_eq!(encoded.matches(BRACKETED_PASTE_END).count(), 1);
        let pasted = encoded
            .strip_prefix(BRACKETED_PASTE_START)
            .unwrap()
            .strip_suffix(BRACKETED_PASTE_END)
            .unwrap();
        assert_eq!(pasted.matches("<domios ").count(), 1);
        assert_eq!(pasted.matches("</domios>").count(), 1);
        let parsed = codeterm_wire::domios::parse_message(pasted).unwrap();
        assert_eq!(parsed.attr("from"), Some(from));
        assert_eq!(parsed.body, body);
        assert!(!pasted.contains('\x1b'));
        assert_eq!(
            parsed.content,
            format!(
                "{body}{}{}",
                codeterm_wire::domios::pasted_marker_tail(),
                frame.to_frame().suffix
            )
        );
        if let Some(context) = envelope.context.as_deref() {
            assert_eq!(parsed.attr("context"), Some(context));
            assert!(!pasted.contains("<tip>"));
        }
        if from == "cli" {
            assert_eq!(frame.to_frame().opening_tag, "<domios from=\"cli\">");
            assert!(!pasted.contains("<tip>"));
        }
    }
}

#[test]
fn human_input_is_one_unframed_paste() {
    let input = crate::service::messenger::AgentInput::Human(
        crate::service::messenger::HumanInput::composer("opaque human alpha\nopaque human beta"),
    );
    let rendered = input.text();
    let body = rendered.as_ref();
    let bytes = encode_paste_payload(
        body,
        false,
        None,
        Some(false),
        PasteEncodingCapability::MultilineTui,
    );
    assert_eq!(
        bytes,
        format!("{BRACKETED_PASTE_START}{body}{BRACKETED_PASTE_END}").as_bytes()
    );
}

#[test]
fn restored_framed_delivery_keeps_existing_marker_and_tip_in_one_paste() {
    use crate::service::messenger::{MessageEnvelope, Messenger};
    let frame =
        Messenger::from(MessageEnvelope::tab("Sender", "source")).framed_delivery("opaque\nbody");
    let original = codeterm_wire::domios::with_pasted_marker(&frame.to_message()).unwrap();
    let restored = Messenger::restore_framed_delivery(&original).unwrap();
    let capability = serde_json::from_value(serde_json::json!({
        "supportsBracketedPaste": true, "marksRoutedMessages": true
    }))
    .unwrap();
    let input =
        crate::service::pty_io::send::prepare_routed_input(&restored.to_frame(), Some(&capability));
    assert_eq!(input.body, original);
    let bytes = encode_paste_payload(
        &input.body,
        false,
        None,
        Some(false),
        PasteEncodingCapability::MultilineTui,
    );
    assert_eq!(
        bytes,
        format!("{BRACKETED_PASTE_START}{original}{BRACKETED_PASTE_END}").as_bytes()
    );
}
