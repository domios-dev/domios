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
fn complete_routed_frame_is_one_bracketed_insertion_with_both_tags() {
    for body in ["opaque body", "opaque first line\nopaque second line"] {
        let frame = codeterm_types::domios::DomiosFrame {
            opening_tag: codeterm_types::domios::build_opening_tag(&[
                ("from", "tab"),
                ("tab", "abc12345"),
            ]),
            body: body.into(),
            suffix: " [opaque hint]".into(),
        };
        let message = frame.to_message();
        let encoded = encode_paste_frame(
            &message,
            None,
            None,
            Some(true),
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
        assert_eq!(semantics.provider_rows, vec![message]);
    }
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
                let frame = encode_paste_frame(body, prefix, suffix, live_mode, capability);
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
            encode_paste_frame("opaque", None, None, live_mode, capability),
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
