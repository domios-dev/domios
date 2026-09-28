use super::*;

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
    assert_eq!(p, "-=-codeterm:tab label=Edison tab=e8a0ac20-=-");
}

#[test]
fn enriched_message_orders_segments() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .model(Some("opus".into()))
        .mesh(Some("Mac-Air".into()))
        .build();
    assert_eq!(
        p,
        "-=-codeterm:tab label=Edison tab=e8a0ac20 model=opus mesh=Mac-Air-=-"
    );
}

#[test]
fn inbound_webhook_pebble_pty_and_chatbackend_metadata_are_originated_as_pebble() {
    let envelope = MessageEnvelope::pebble();
    let marker = Messenger::from(envelope.clone()).build();
    let parsed = crate::message_markers::parse(&marker).expect("pebble marker parses");
    assert_eq!(parsed.kind, "pebble");
    assert_eq!(parsed.attr("source"), Some("Pebble"));
    assert_eq!(parsed.attr("trust"), None);

    let (msg, marked) = build_chatbackend_inbound(&envelope, "focus enabled", "pebble-1".into());
    assert_eq!(msg.inbound_kind.as_deref(), Some("pebble"));
    assert_eq!(msg.from_label.as_deref(), Some("Pebble"));
    let parsed = crate::message_markers::parse(&marked).expect("inbound marker parses");
    assert_eq!(parsed.attr("kind"), Some("pebble"));
    assert_eq!(parsed.attr("trust"), None);
    assert_eq!(parsed.payload, "focus enabled");
}

#[test]
fn plugin_icon_svg_webhook_metadata_preserves_id_and_sanitized_icon_for_chatbackend() {
    let icon = "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>";
    let envelope = MessageEnvelope::plugin("pebble", Some(icon.into()));
    let marker = Messenger::from(envelope.clone()).build();
    let parsed = crate::message_markers::parse(&marker).expect("plugin marker parses");
    assert_eq!(parsed.kind, "pebble");
    assert_eq!(parsed.attr("plugin"), Some("pebble"));

    let (msg, marked) = build_chatbackend_inbound(&envelope, "focus enabled", "plugin-1".into());
    assert_eq!(msg.inbound_kind.as_deref(), Some("plugin"));
    assert_eq!(msg.from_plugin.as_deref(), Some("pebble"));
    assert_eq!(msg.plugin_icon_svg.as_deref(), Some(icon));
    let parsed = crate::message_markers::parse(&marked).expect("inbound marker parses");
    assert_eq!(parsed.attr("kind"), Some("plugin"));
    assert_eq!(parsed.attr("from_plugin"), Some("pebble"));
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
        "-=-codeterm:report from=Worker tab=abc12345 model=sonnet-=-"
    );
}

#[test]
fn empty_model_and_mesh_are_dropped() {
    let p = Messenger::message("A", "deadbeef")
        .model(Some("  ".into()))
        .mesh(Some(String::new()))
        .build();
    assert_eq!(p, "-=-codeterm:tab label=A tab=deadbeef-=-");
}

#[test]
fn brackets_in_name_are_stripped() {
    let p = Messenger::message("Main [dev]", "abcdef12").build();
    assert_eq!(p, "-=-codeterm:tab label=Main%20dev tab=abcdef12-=-");
}

#[test]
fn wrap_appends_body() {
    let p = Messenger::message("Edison", "e8a0ac20").from_agent(Some(true));
    let wrapped = p.wrap("hello");
    assert!(wrapped.starts_with("-=-codeterm:tab label=Edison tab=e8a0ac20-=-\nhello\n"));
    let hint = wrapped.rsplit('\n').next().unwrap();
    assert!(hint.starts_with(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(hint.contains(Messenger::AGENT_REPLY_COMMAND));
    assert!(hint.contains("--tab e8a0ac20"));
}

#[test]
fn mesh_reply_hint_carries_the_return_peer() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .from_agent(Some(true))
        .mesh(Some("BLACKSTAR".into()));
    let wrapped = p.wrap("hello");
    assert!(wrapped.starts_with("-=-codeterm:tab label=Edison tab=e8a0ac20 mesh=BLACKSTAR-=-\nhello\n"));
    assert!(wrapped.ends_with("--tab e8a0ac20 --mesh BLACKSTAR]"));
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
        let hint = wrapped.rsplit('\n').next().unwrap();
        assert_eq!(hint, Messenger::SHELL_REPLY_HINT);
    }
}

#[test]
fn reply_hint_for_agent_sender_unchanged() {
    let wrapped = Messenger::message("Agent", "e8a0ac20").wrap("hello");
    let hint = wrapped.rsplit('\n').next().unwrap();
    assert!(hint.starts_with(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(hint.contains(Messenger::AGENT_REPLY_COMMAND));
    assert!(hint.contains("--tab e8a0ac20"));
}

#[test]
fn codeterm_system_events_have_no_tab_or_reply_address() {
    let plain = Messenger::from(MessageEnvelope::from_codeterm()).wrap("hello");
    assert_eq!(plain, "-=-codeterm:system-=-\nhello");
    assert!(!plain.contains("tab="));
    assert!(!plain.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    assert!(!plain.contains(Messenger::SHELL_REPLY_HINT));

    let contextual = Messenger::from(MessageEnvelope::codeterm_reminder("plan")).wrap("check");
    assert_eq!(contextual, "-=-codeterm:system context=plan-=-\ncheck");
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
    let parsed = crate::message_markers::parse(&marked).expect("inbound marker parses");
    assert_eq!(parsed.kind, "inbound");
    assert_eq!(parsed.attr("kind"), Some("tab"));
    assert_eq!(parsed.attr("from_label"), Some("Chat Backend"));
    assert_eq!(parsed.attr("from_tab"), Some("abc12345"));
    assert_eq!(parsed.attr("from_model"), Some("qwen"));
    assert_eq!(parsed.attr("from_mesh"), Some("mesh-a"));
    assert_eq!(parsed.payload, "hello");
}

#[test]
fn peer_qualifies_label() {
    let p = Messenger::message("Edison", "e8a0ac20")
        .peer(Some("BLACKSTAR".into()))
        .build();
    assert_eq!(p, "-=-codeterm:tab label=Edison@BLACKSTAR tab=e8a0ac20-=-");
}

#[test]
fn peer_without_label_uses_peer_alone() {
    // No distinct source label → don't render the silly `PEER@PEER`.
    let p = Messenger::message("", "e8a0ac20")
        .peer(Some("BLACKSTAR".into()))
        .build();
    assert_eq!(p, "-=-codeterm:tab label=BLACKSTAR tab=e8a0ac20-=-");
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
        "-=-codeterm:reminder from=Main%20dev tab=e8a0ac20 model=opus set_at=1000 fire_at=2000 id=dmsg-abc-=-"
    );
}

#[test]
fn reminder_drops_unknown_provenance() {
    let p = Messenger::reminder("", "").set_at(5).fire_at(9).build();
    assert_eq!(p, "-=-codeterm:reminder set_at=5 fire_at=9-=-");
}

#[test]
fn reminder_wraps_body() {
    let p = Messenger::reminder("Main", "e8a0ac20")
        .set_at(1)
        .fire_at(2)
        .id(Some("dmsg-1".into()));
    assert_eq!(
        p.wrap("ping"),
        "-=-codeterm:reminder from=Main tab=e8a0ac20 set_at=1 fire_at=2 id=dmsg-1-=-\nping"
    );
}

// --- delivery_prefix: one canonical decision per delivery path ---

#[test]
fn delivery_local_agent_recipient_gets_prefix() {
    let p = delivery_prefix(true, "e8a0ac20", "ce9bce56", "Edison", Some("opus"), None);
    assert_eq!(
        p.as_deref(),
        Some("-=-codeterm:tab label=Edison tab=e8a0ac20 model=opus-=-")
    );
}

#[test]
fn delivery_shell_recipient_gets_none() {
    // Shells run bytes verbatim — no source tag, local or mesh.
    assert_eq!(
        delivery_prefix(false, "e8a0ac20", "ce9bce56", "Edison", None, None),
        None
    );
    assert_eq!(
        delivery_prefix(
            false,
            "e8a0ac20",
            "ce9bce56",
            "Edison",
            None,
            Some("BLACKSTAR")
        ),
        None
    );
}

#[test]
fn delivery_mesh_agent_recipient_gets_peer_qualified_prefix() {
    let p = delivery_prefix(
        true,
        "e8a0ac20",
        "ce9bce56",
        "Edison",
        Some("sonnet"),
        Some("BLACKSTAR"),
    );
    assert_eq!(
        p.as_deref(),
        Some("-=-codeterm:tab label=Edison@BLACKSTAR tab=e8a0ac20 model=sonnet mesh=BLACKSTAR-=-")
    );
}

#[test]
fn delivery_self_send_gets_none() {
    assert_eq!(
        delivery_prefix(true, "e8a0ac20", "e8a0ac20", "Edison", None, None),
        None
    );
}

#[test]
fn delivery_empty_source_gets_none() {
    assert_eq!(
        delivery_prefix(true, "", "ce9bce56", "Edison", None, None),
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
    let parsed = crate::message_markers::parse(&ctx).expect("reminder inbound marker parses");
    let legacy = ["[CodeTerm", " inbound"].concat();

    assert!(!ctx.contains(&legacy));
    assert_eq!(parsed.kind, "inbound");
    assert_eq!(parsed.attr("kind"), Some("reminder"));
    assert_eq!(parsed.attr("from_label"), Some("Main"));
    assert_eq!(parsed.attr("from_tab"), Some("e8a0ac20"));
    assert_eq!(parsed.attr("from_model"), Some("opus"));
    assert_eq!(parsed.payload, "ping");
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
    assert!(marked.contains("-=-codeterm:inbound"));
    assert!(marked.contains("from_label=Gauss"));
    assert!(marked.contains("ship it"));
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
    let parsed = crate::message_markers::parse(&provider_input[0]).unwrap();
    assert_eq!(parsed.kind, "inbound");
    assert_eq!(parsed.attr("kind"), Some("system"));
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
    let parsed = crate::message_markers::parse(&ctx).expect("marker parses");
    let legacy = ["[CodeTerm", " inbound"].concat();

    assert!(!ctx.contains(&legacy));
    assert_eq!(parsed.kind, "inbound");
    assert_eq!(parsed.attr("kind"), Some("tab"));
    assert_eq!(parsed.attr("from_label"), Some("Edison"));
    assert_eq!(parsed.attr("from_tab"), Some("e8a0ac20"));
    assert_eq!(parsed.attr("from_model"), Some("opus"));
    assert_eq!(parsed.attr("from_mesh"), Some("BLACKSTAR"));
    assert_eq!(parsed.payload, "hello");
}
