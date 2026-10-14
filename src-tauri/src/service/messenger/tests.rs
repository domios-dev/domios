use super::*;

#[tokio::test]
async fn codex_rpc_send_messenger_preserves_routed_frame_and_queues_while_active() {
    use crate::service::codex_rpc::send::{route_or_pty, start_params, submit, Origin};
    let frame =
        Messenger::from(MessageEnvelope::tab("sender", "sender-a")).routed_frame("opaque payload");
    let text = frame.to_message();
    let params = start_params("thread-a", "client-a", &text, None, serde_json::json!({}));
    let mut writes = Vec::new();
    let result = submit(Origin::Agent, Some("active-a"), params, |method, params| {
        writes.push((method, params));
        std::future::ready(Ok(serde_json::json!({"turn":{"startedAt":null}})))
    })
    .await;
    route_or_pty(Some(result.map(|_| ())), || {
        panic!("messenger must not paste after RPC")
    })
    .unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, "turn/start");
    assert_eq!(writes[0].1["clientUserMessageId"], "client-a");
    assert_eq!(writes[0].1["input"][0]["text"], text);
    assert!(text.starts_with(&frame.opening_tag));
    assert!(text.ends_with(codeterm_types::domios::DOMIOS_CLOSE_TAG));
}

#[test]
fn every_pty_envelope_uses_the_shared_submit_transaction() {
    assert_eq!(
        pty_submit_entry(MessageKind::Message),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::Reminder),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::System),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::Mesh),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::Telegram),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::Pebble),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger
    );
    assert_eq!(
        pty_submit_entry(MessageKind::Report),
        crate::service::pty_io::submit_transaction::PtySubmitEntry::Report
    );
}

#[test]
fn bare_message_tags_codeterm() {
    let p = Messenger::message("Edison", "e8a0ac20-1111").build();
    assert_eq!(p, "<domios from=\"tab\" label=\"Edison\" tab=\"e8a0ac20\">");
}

#[test]
fn enriched_message_orders_segments() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .model(Some("opus".into()))
        .mesh(Some("Mac-Air".into()))
        .build();
    assert_eq!(
        p,
        "<domios from=\"tab\" label=\"Edison\" tab=\"e8a0ac20\" model=\"opus\" mesh=\"Mac-Air\">"
    );
}

#[test]
fn inbound_webhook_pebble_pty_and_chatbackend_metadata_are_originated_as_pebble() {
    let envelope = MessageEnvelope::pebble();
    let marker = Messenger::from(envelope.clone()).wrap("focus enabled");
    let parsed = codeterm_types::domios::parse_message(&marker).expect("pebble frame parses");
    assert_eq!(parsed.attr("from"), Some("pebble"));
    assert_eq!(parsed.attr("source"), Some("Pebble"));
    assert_eq!(parsed.attr("trust"), None);
    assert_eq!(parsed.body, "focus enabled");

    let (msg, marked) = build_chatbackend_inbound(&envelope, "focus enabled", "pebble-1".into());
    assert_eq!(msg.inbound_kind.as_deref(), Some("pebble"));
    assert_eq!(msg.from_label.as_deref(), Some("Pebble"));
    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("from"), Some("pebble"));
    assert_eq!(parsed.attr("trust"), None);
    assert_eq!(parsed.body, "focus enabled");
}

#[test]
fn plugin_icon_svg_webhook_metadata_preserves_id_and_sanitized_icon_for_chatbackend() {
    let icon = "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>";
    let envelope = MessageEnvelope::plugin("pebble", Some(icon.into()));
    let marker = Messenger::from(envelope.clone()).wrap("focus enabled");
    let parsed = codeterm_types::domios::parse_message(&marker).expect("plugin frame parses");
    assert_eq!(parsed.attr("from"), Some("pebble"));
    assert_eq!(parsed.attr("plugin"), Some("pebble"));
    assert_eq!(parsed.body, "focus enabled");

    let (msg, marked) = build_chatbackend_inbound(&envelope, "focus enabled", "plugin-1".into());
    assert_eq!(msg.inbound_kind.as_deref(), Some("plugin"));
    assert_eq!(msg.from_plugin.as_deref(), Some("pebble"));
    assert_eq!(msg.plugin_icon_svg.as_deref(), Some(icon));
    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("from"), Some("plugin"));
    assert_eq!(parsed.attr("plugin"), Some("pebble"));
}

#[test]
fn inbound_webhook_pebble_pty_delivery_marks_an_agent_even_without_a_source_tab() {
    assert!(should_prefix_pty(MessageKind::Pebble, true, "", "general"));
    assert!(!should_prefix_pty(MessageKind::Pebble, false, "", "shell"));
}

#[test]
fn report_uses_report_lead() {
    let p = Messenger::report("Worker", "abc12345")
        .model(Some("sonnet".into()))
        .build();
    assert_eq!(
        p,
        "<domios from=\"report\" label=\"Worker\" tab=\"abc12345\" model=\"sonnet\">"
    );
}

#[test]
fn empty_model_and_mesh_are_dropped() {
    let p = Messenger::message("A", "deadbeef")
        .model(Some("  ".into()))
        .mesh(Some(String::new()))
        .build();
    assert_eq!(p, "<domios from=\"tab\" label=\"A\" tab=\"deadbeef\">");
}

#[test]
fn brackets_in_name_are_stripped() {
    let p = Messenger::message("Main [dev]", "abcdef12").build();
    assert_eq!(
        p,
        "<domios from=\"tab\" label=\"Main dev\" tab=\"abcdef12\">"
    );
}

#[test]
fn wrap_appends_body() {
    let p = Messenger::message("Edison", "e8a0ac20").from_agent(Some(true));
    let wrapped = p.wrap("hello");
    assert!(wrapped.starts_with("<domios from=\"tab\" label=\"Edison\" tab=\"e8a0ac20\">hello "));
    assert!(wrapped.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(wrapped.contains(Messenger::AGENT_REPLY_COMMAND));
    assert!(wrapped.contains("--tab e8a0ac20"));
    assert!(wrapped.ends_with(codeterm_types::domios::DOMIOS_CLOSE_TAG));
}

#[test]
fn mesh_reply_hint_carries_the_return_peer() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .from_agent(Some(true))
        .mesh(Some("BLACKSTAR".into()));
    let wrapped = p.wrap("hello");
    assert!(wrapped.starts_with(
        "<domios from=\"tab\" label=\"Edison\" tab=\"e8a0ac20\" mesh=\"BLACKSTAR\">hello "
    ));
    assert!(wrapped.contains("--tab e8a0ac20 --mesh BLACKSTAR]"));
    assert!(wrapped.ends_with(codeterm_types::domios::DOMIOS_CLOSE_TAG));
}

#[test]
fn reply_hint_for_shell_sender_suggests_no_shell_send() {
    for envelope in [
        MessageEnvelope::tab("Shell", "e8a0ac20").from_agent(Some(false)),
        MessageEnvelope::tab("Shell", "e8a0ac20")
            .from_agent(Some(false))
            .mesh(Some("peer".into())),
    ] {
        let wrapped = Messenger::from(envelope).wrap("hello");
        assert!(wrapped.contains(Messenger::SHELL_REPLY_HINT));
        assert!(wrapped.ends_with(codeterm_types::domios::DOMIOS_CLOSE_TAG));
    }
}

#[test]
fn reply_hint_for_agent_sender_unchanged() {
    let wrapped = Messenger::message("Agent", "e8a0ac20").wrap("hello");
    assert!(wrapped.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(wrapped.contains(Messenger::AGENT_REPLY_COMMAND));
    assert!(wrapped.contains("--tab e8a0ac20"));
}

#[test]
fn codeterm_system_events_have_no_tab_or_reply_address() {
    let plain = Messenger::from(MessageEnvelope::from_codeterm()).wrap("hello");
    assert_eq!(plain, "<domios from=\"system\">hello</domios>");
    assert!(!plain.contains("tab="));
    assert!(!plain.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!plain.contains(Messenger::SHELL_REPLY_HINT));

    let contextual = Messenger::from(MessageEnvelope::codeterm_reminder("plan")).wrap("check");
    assert_eq!(
        contextual,
        "<domios from=\"system\" context=\"plan\">check</domios>"
    );
    assert!(!contextual.contains("tab="));
    assert!(!contextual.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!contextual.contains(Messenger::SHELL_REPLY_HINT));
}

#[test]
fn reports_do_not_request_a_reply() {
    let wrapped = Messenger::report("Worker", "abc12345").wrap("done");
    assert!(!wrapped.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!wrapped.contains(Messenger::SHELL_REPLY_HINT));
}

#[test]
fn inbound_model_context_uses_canonical_marker() {
    let envelope = MessageEnvelope::tab("Chat Backend", "abc12345-ffff")
        .model(Some("qwen".into()))
        .mesh(Some("mesh-a".into()));
    let marked = inbound_model_context(&envelope, "hello");
    let legacy = ["[CodeTerm", " inbound"].concat();

    assert!(!marked.contains(&legacy));
    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("from"), Some("tab"));
    assert_eq!(parsed.attr("label"), Some("Chat Backend"));
    assert_eq!(parsed.attr("tab"), Some("abc12345"));
    assert_eq!(parsed.attr("model"), Some("qwen"));
    assert_eq!(parsed.attr("mesh"), Some("mesh-a"));
    assert_eq!(parsed.body, "hello");
}

#[test]
fn peer_qualifies_label() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .peer(Some("BLACKSTAR".into()))
        .build();
    assert_eq!(
        p,
        "<domios from=\"tab\" label=\"Edison@BLACKSTAR\" tab=\"e8a0ac20\">"
    );
}

#[test]
fn peer_without_label_uses_peer_alone() {
    // No distinct source label → don't render the silly `PEER@PEER`.
    let p = Messenger::message("", "e8a0ac20")
        .peer(Some("BLACKSTAR".into()))
        .build();
    assert_eq!(
        p,
        "<domios from=\"tab\" label=\"BLACKSTAR\" tab=\"e8a0ac20\">"
    );
}

#[test]
fn reminder_emits_provenance_attrs() {
    let p = Messenger::reminder("Main [dev]", "e8a0ac20-1111")
        .model(Some("opus".into()))
        .set_at(1000)
        .fire_at(2000)
        .id(Some("dmsg-abc".into()))
        .build();
    assert_eq!(
        p,
        "<domios from=\"reminder\" label=\"Main dev\" tab=\"e8a0ac20\" model=\"opus\" set_at=\"1000\" fire_at=\"2000\" id=\"dmsg-abc\">"
    );
}

#[test]
fn reminder_drops_unknown_provenance() {
    let p = Messenger::reminder("", "").set_at(5).fire_at(9).build();
    assert_eq!(p, "<domios from=\"reminder\" set_at=\"5\" fire_at=\"9\">");
}

#[test]
fn reminder_wraps_body() {
    let p = Messenger::reminder("Main", "e8a0ac20")
        .set_at(1)
        .fire_at(2)
        .id(Some("dmsg-1".into()));
    assert_eq!(
        p.wrap("ping"),
        "<domios from=\"reminder\" label=\"Main\" tab=\"e8a0ac20\" set_at=\"1\" fire_at=\"2\" id=\"dmsg-1\">ping</domios>"
    );
}

// --- delivery_message: one canonical decision per delivery path ---

#[test]
fn delivery_local_agent_recipient_gets_complete_frame() {
    let p = delivery_message(
        true,
        "e8a0ac20",
        "ce9bce56",
        "Edison",
        Some("opus"),
        None,
        true,
        "hello",
    );
    assert_eq!(
        p.as_deref(),
        Some("<domios from=\"tab\" label=\"Edison\" tab=\"e8a0ac20\" model=\"opus\">hello <tip>[to reply, run: codeterm send \"YOUR REPLY\" --tab e8a0ac20]</tip></domios>")
    );
}

#[test]
fn delivery_shell_recipient_gets_none() {
    // Shells run bytes verbatim — no source tag, local or mesh.
    assert_eq!(
        delivery_message(false, "e8a0ac20", "ce9bce56", "Edison", None, None, true, "hello"),
        None
    );
    assert_eq!(
        delivery_message(
            false,
            "e8a0ac20",
            "ce9bce56",
            "Edison",
            None,
            Some("BLACKSTAR"),
            true,
            "hello",
        ),
        None
    );
}

#[test]
fn delivery_mesh_agent_recipient_gets_peer_qualified_frame() {
    let p = delivery_message(
        true,
        "e8a0ac20",
        "ce9bce56",
        "Edison",
        Some("sonnet"),
        Some("BLACKSTAR"),
        true,
        "hello",
    );
    assert_eq!(
        p.as_deref(),
        Some("<domios from=\"tab\" label=\"Edison@BLACKSTAR\" tab=\"e8a0ac20\" model=\"sonnet\" mesh=\"BLACKSTAR\">hello <tip>[to reply, run: codeterm send \"YOUR REPLY\" --tab e8a0ac20 --mesh BLACKSTAR]</tip></domios>")
    );
}

#[test]
fn typed_route_parts_stay_on_one_line_for_untrusted_attributes() {
    let frame = Messenger::message("Sender\nInjected", "abc12345")
        .model(Some("model\rInjected".into()))
        .mesh(Some("PEER\nInjected".into()))
        .routed_frame("opaque body");

    assert!(!frame.opening_tag.contains('\n'));
    assert!(!frame.opening_tag.contains('\r'));
    assert!(frame.suffix.is_empty());
}

#[test]
fn delivery_self_send_gets_none() {
    assert_eq!(
        delivery_message(true, "e8a0ac20", "e8a0ac20", "Edison", None, None, true, "hello"),
        None
    );
}

#[test]
fn delivery_empty_source_gets_none() {
    assert_eq!(
        delivery_message(true, "", "ce9bce56", "Edison", None, None, true, "hello"),
        None
    );
}

#[test]
fn reminder_inbound_context_is_canonical_with_sender_fields() {
    let envelope = MessageEnvelope::reminder("Main", "e8a0ac20-1111")
        .model(Some("opus".into()))
        .set_at(1000)
        .fire_at(2000)
        .id(Some("dmsg-1".into()));
    let ctx = inbound_model_context(&envelope, "ping");
    let parsed = codeterm_types::domios::parse_message(&ctx).expect("reminder inbound tag parses");
    let legacy = ["[CodeTerm", " inbound"].concat();

    assert!(!ctx.contains(&legacy));
    assert_eq!(parsed.attr("from"), Some("reminder"));
    assert_eq!(parsed.attr("label"), Some("Main"));
    assert_eq!(parsed.attr("tab"), Some("e8a0ac20"));
    assert_eq!(parsed.attr("model"), Some("opus"));
    assert_eq!(parsed.body, "ping");
}

#[test]
fn should_prefix_reminder_always_marks_agent_recipient() {
    // Self-addressed reminder (creator == target) still gets the marker.
    assert!(should_prefix_pty(
        MessageKind::Reminder,
        true,
        "e8a0ac20",
        "e8a0ac20"
    ));
    // Unknown-creator reminder still gets the marker.
    assert!(should_prefix_pty(
        MessageKind::Reminder,
        true,
        "",
        "e8a0ac20"
    ));
}

#[test]
fn should_prefix_reminder_skips_shell_recipient() {
    // Shells run injected bytes verbatim — no marker, even for a reminder.
    assert!(!should_prefix_pty(
        MessageKind::Reminder,
        false,
        "abc",
        "e8a0ac20"
    ));
}

#[test]
fn should_prefix_system_marks_only_agent_recipients() {
    assert!(should_prefix_pty(MessageKind::System, true, "", "e8a0ac20"));
    assert!(!should_prefix_pty(
        MessageKind::System,
        false,
        "",
        "e8a0ac20"
    ));
}

#[test]
fn should_prefix_message_keeps_self_send_guard() {
    // A plain message to itself is NOT prefixed.
    assert!(!should_prefix_pty(
        MessageKind::Message,
        true,
        "e8a0ac20",
        "e8a0ac20"
    ));
    // Empty source → no prefix.
    assert!(!should_prefix_pty(
        MessageKind::Message,
        true,
        "",
        "e8a0ac20"
    ));
    // Distinct agent sender → prefix.
    assert!(should_prefix_pty(
        MessageKind::Message,
        true,
        "e8a0ac20",
        "ce9bce56"
    ));
}

#[test]
fn chatbackend_inbound_stores_clean_body_model_gets_marker() {
    // Stored row must be the clean body (no inline marker); the marker goes only to the model-facing string.
    let envelope = MessageEnvelope::tab("Gauss", "abc12345-ffff")
        .model(Some("qwen".into()))
        .mesh(Some("Mac-Air".into()));
    let (msg, marked) = build_chatbackend_inbound(&envelope, "ship it", "inbound-x".into());

    assert_eq!(msg.content, "ship it", "stored row is the clean body");
    assert!(
        !msg.content.contains("-=-codeterm"),
        "no raw marker leaks into the rendered body"
    );
    assert_eq!(msg.kind, "user");
    assert_eq!(msg.from_label.as_deref(), Some("Gauss"));
    assert_eq!(msg.from_tab.as_deref(), Some("abc12345"));
    assert_eq!(msg.from_model.as_deref(), Some("qwen"));
    assert_eq!(msg.from_mesh.as_deref(), Some("Mac-Air"));
    assert_eq!(msg.inbound_kind.as_deref(), Some("tab"));

    // The model still learns the sender + return address via the marker.
    assert!(marked.starts_with("<domios from=\"tab\""));
    assert!(marked.contains("label=\"Gauss\""));
    assert!(marked.contains("ship it"));
}

#[test]
fn chatbackend_system_envelope_carries_source_label_without_address() {
    let body = "opaque body: $x\nsecond line";
    let envelope = MessageEnvelope::from_codeterm();

    let origin = Messenger::from(envelope.clone()).wrap(body);
    assert_eq!(origin, format!("<domios from=\"system\">{body}</domios>"));
    assert!(!origin.contains("tab="));
    assert!(!origin.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!origin.contains(Messenger::SHELL_REPLY_HINT));

    let (msg, marked) = build_chatbackend_inbound(&envelope, body, "system-inbound".into());
    assert_eq!(msg.content, body);
    assert_eq!(msg.inbound_kind.as_deref(), Some("system"));
    assert_eq!(msg.from_label.as_deref(), Some(MessageEnvelope::CODETERM));
    assert!(msg.from_tab.is_none());

    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("from"), Some("system"));
    assert_eq!(parsed.attr("label"), Some(MessageEnvelope::CODETERM));
    assert_eq!(parsed.attr("tab"), None);
    assert_eq!(parsed.body, body);
    assert!(!marked.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!marked.contains(Messenger::SHELL_REPLY_HINT));
}

#[test]
fn utility_shell_purpose_follows_tab_in_pty_and_chatbackend_frames() {
    let envelope = MessageEnvelope::tab("Builder", "abc12345-ffff")
        .purpose(Some("build".into()))
        .from_agent(Some(false));
    assert_eq!(
        Messenger::from(envelope.clone()).build(),
        "<domios from=\"tab\" label=\"Builder\" tab=\"abc12345\" purpose=\"build\">"
    );
    assert_eq!(
        Messenger::message("Builder", "abc12345")
            .purpose(Some("  ".into()))
            .build(),
        "<domios from=\"tab\" label=\"Builder\" tab=\"abc12345\">"
    );

    let (msg, marked) = build_chatbackend_inbound(&envelope, "cargo ok", "purpose-1".into());
    assert_eq!(msg.from_purpose.as_deref(), Some("build"));
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["fromPurpose"], "build");
    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("purpose"), Some("build"));

    let (plain, plain_marked) = build_chatbackend_inbound(
        &MessageEnvelope::tab("Shell", "abc12345"),
        "ls",
        "purpose-2".into(),
    );
    assert!(plain.from_purpose.is_none());
    assert!(!plain_marked.contains("purpose="));
}

#[test]
fn chatbackend_system_row_keeps_context() {
    let envelope = MessageEnvelope::codeterm_reminder("blueprint:bp-1:result");
    let (msg, marked) = build_chatbackend_inbound(&envelope, "done", "system-ctx".into());
    assert_eq!(msg.from_context.as_deref(), Some("blueprint:bp-1:result"));
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["fromContext"], "blueprint:bp-1:result");
    let parsed = codeterm_types::domios::parse_message(&marked).expect("inbound tag parses");
    assert_eq!(parsed.attr("context"), Some("blueprint:bp-1:result"));

    let (bare, _) =
        build_chatbackend_inbound(&MessageEnvelope::from_codeterm(), "x", "system-bare".into());
    assert!(bare.from_context.is_none());
}

#[test]
fn chatbackend_planner_payload_reaches_provider_input_as_data() {
    let payload = "- audit `model`\n- <nonexistent>\n$env:X='y'; $(false)";
    let envelope = MessageEnvelope::codeterm_reminder("planner-request");
    let (_, marked) = build_chatbackend_inbound(&envelope, payload, "planner-request-1".into());
    let mut provider_input = Vec::new();
    submit_chatbackend_provider_input(&marked, |input| {
        provider_input.push(input.to_string());
        Ok(())
    })
    .unwrap();
    assert_eq!(provider_input.len(), 1);
    assert!(provider_input[0].contains(payload));
    let parsed = codeterm_types::domios::parse_message(&provider_input[0]).unwrap();
    assert_eq!(parsed.attr("from"), Some("system"));
    assert_eq!(parsed.attr("context"), Some("planner-request"));
    assert_eq!(parsed.attr("from_tab"), None);
}

#[test]
fn chat_backend_delivery_bypasses_pty_submit() {
    let mut provider_inputs = Vec::new();
    let mut pty_submits = 0;
    dispatch_delivery(
        DeliveryTransport::ChatBackend,
        || {
            submit_chatbackend_provider_input("hello", |input| {
                provider_inputs.push(input.to_string());
                Ok::<_, String>(())
            })
        },
        || {
            pty_submits += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(provider_inputs, ["hello"]);
    assert_eq!(pty_submits, 0);
}

#[test]
fn chatbackend_rotation_after_preflight_is_not_accepted_as_delivered() {
    let opened = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "provider".into(),
        session_id: "opened-session".into(),
    };
    assert!(planner_chatbackend_binding_matches(
        Some("opened-session"),
        Some("opened-nonce"),
        Some(&opened),
        "opened-session",
        "opened-nonce",
        "provider",
    ));

    let rotated = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "provider".into(),
        session_id: "rotated-session".into(),
    };
    assert!(!planner_chatbackend_binding_matches(
        Some("rotated-session"),
        Some("rotated-nonce"),
        Some(&rotated),
        "opened-session",
        "opened-nonce",
        "provider",
    ));
}

#[test]
fn inbound_context_has_sender_with_canonical_marker() {
    let envelope = MessageEnvelope::tab("Edison", "e8a0ac20-1111")
        .model(Some("opus".into()))
        .mesh(Some("BLACKSTAR".into()));
    let ctx = inbound_model_context(&envelope, "hello");
    let parsed = codeterm_types::domios::parse_message(&ctx).expect("tag parses");
    let legacy = ["[CodeTerm", " inbound"].concat();

    assert!(!ctx.contains(&legacy));
    assert_eq!(parsed.attr("from"), Some("tab"));
    assert_eq!(parsed.attr("label"), Some("Edison"));
    assert_eq!(parsed.attr("tab"), Some("e8a0ac20"));
    assert_eq!(parsed.attr("model"), Some("opus"));
    assert_eq!(parsed.attr("mesh"), Some("BLACKSTAR"));
    assert_eq!(parsed.body, "hello");
}

#[test]
fn relay_metadata_preserves_sender_and_hop_count() {
    let mut envelope = MessageEnvelope::tab("relay", "abcdef12-3456-7890-abcd-123456789abc");
    envelope.relay_hops = 3;
    let marked = Messenger::from(envelope).wrap("opaque answer");
    let parsed = codeterm_types::domios::parse_message(&marked).unwrap();
    assert_eq!(parsed.attr("tab"), Some("abcdef12"));
    assert_eq!(parsed.attr("relay_hops"), Some("3"));
    assert_eq!(parsed.body, "opaque answer");
}

#[test]
fn routed_hint_is_an_agent_only_tip_for_local_mesh_and_shell_senders() {
    for messenger in [
        Messenger::message("Sender", "abc12345"),
        Messenger::message("Sender", "abc12345").mesh(Some("peer".into())),
        Messenger::message("Sender", "abc12345").from_agent(Some(false)),
    ] {
        let frame = messenger.routed_frame("opaque body");
        let hint = messenger.reply_hint().unwrap();
        assert_eq!(frame.suffix, format!(" <tip>{hint}</tip>"));
        let parsed = codeterm_types::domios::parse_message(&frame.to_message()).unwrap();
        assert_eq!(parsed.body, frame.body);
        assert_eq!(parsed.content, format!("{}{}", frame.body, frame.suffix));
    }
}
