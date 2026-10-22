use super::{gateway_send_response, local_send_response, require_send_target};
use codeterm_service_core::service::delivery_error::DeliveryError;
use codeterm_service_core::service::messenger::Delivery;

#[test]
fn socket_send_notice_keeps_build_provenance() {
    use codeterm_service_core::service::messenger::{MessageEnvelope, Messenger};
    for source in [
        MessageEnvelope::default(),
        MessageEnvelope::tab("Caller", "source"),
    ] {
        let anonymous =
            Messenger::from(super::signed_send_envelope(source.clone(), None)).wrap("opaque body");
        assert!(anonymous.starts_with(if source.from_tab.is_empty() {
            "<domios from=\"cli\">"
        } else {
            "<domios from=\"tab\""
        }));
        let notice = Messenger::from(super::signed_send_envelope(source, Some("build".into())))
            .wrap("opaque body");
        assert_eq!(
            notice,
            "<domios from=\"system\" context=\"build\">opaque body</domios>"
        );
        assert!(!notice.contains("<tip>"));
    }
}

#[test]
fn accepted_runtime_delivery_response_never_claims_zero_write() {
    assert_eq!(
        gateway_send_response(
            Delivery::ChatBackend {
                message_id: "own-send".into()
            },
            None
        ),
        serde_json::json!({"ok":true,"delivery":"sent","bytes_written":true,"send_id":"own-send"})
    );
}
#[test]
fn send_target_must_resolve_to_a_real_tab() {
    let error = require_send_target("system", None).unwrap_err();
    assert_eq!(error.code, -32602);
    assert_eq!(error.message, "Tab not found: system");
    assert_eq!(
        require_send_target("short", Some("full-id".into())).unwrap(),
        "full-id"
    );
}

#[test]
fn local_send_response_reports_confirmed_pending_and_refused_outcomes() {
    assert_eq!(
        local_send_response(Ok(()), true, None),
        serde_json::json!({
            "ok": true,
            "delivery": "sent",
            "bytes_written": true,
        }),
    );
    assert_eq!(
        local_send_response(
            Err(DeliveryError::SubmitUnconfirmed {
                tab: "tab".into(),
                detail: "submit was not confirmed".into(),
            }),
            true,
            None,
        ),
        serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": true,
        }),
    );
    let refused = local_send_response(
        Err(DeliveryError::TabBusyRefused {
            tab: "tab".into(),
            detail: "provider is not ready".into(),
        }),
        true,
        None,
    );
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "tab_busy_refused");
    assert_eq!(refused["error"]["bytes_written"], false);
}

#[test]
fn chat_backend_send_does_not_claim_pty_bytes() {
    assert_eq!(
        local_send_response(Ok(()), false, None),
        serde_json::json!({ "ok": true, "delivery": "sent" }),
    );
}

#[test]
fn enveloped_pty_send_reports_a_completed_write() {
    assert_eq!(
        gateway_send_response(
            Delivery::Pty {
                text: "body".into()
            },
            None
        ),
        serde_json::json!({
            "ok": true,
            "delivery": "sent",
            "bytes_written": true,
        }),
    );
}

#[test]
fn queued_agent_delivery_reports_pending_without_written_bytes() {
    assert_eq!(
        gateway_send_response(
            Delivery::Pending {
                delivery_id: "delivery-1".into(),
            },
            None,
        ),
        serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": false,
        }),
    );
}

#[test]
fn uncertain_runtime_delivery_reports_pending_with_possible_write_and_receipt() {
    assert_eq!(
        gateway_send_response(
            Delivery::Unconfirmed {
                delivery_id: "send-1".into()
            },
            None
        ),
        serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": true,
            "send_id": "send-1",
        }),
    );
}

#[test]
fn socket_send_responses_return_pad_file_for_local_and_enveloped_sends() {
    let path = std::path::PathBuf::from("runtime/agent-pads/tab/inbox/token.md");
    let local = local_send_response(Ok(()), true, Some(path.clone()));
    assert_eq!(local["pad_file"], path.display().to_string());

    let enveloped = gateway_send_response(
        Delivery::Pty {
            text: "pointer".into(),
        },
        Some(path.clone()),
    );
    assert_eq!(enveloped["pad_file"], path.display().to_string());
}

#[cfg(windows)]
#[test]
fn tab_send_text_dispatch_returns_written_recipient_pad_file() {
    use codeterm_service_core::multiplexer::{state::MultiplexerState, types::AgentRole};
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mut mux, sender) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&sender).unwrap();
    let recipient = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&recipient, AgentRole::Worker, Some(&sender))
        .unwrap();
    mux.get_tab_mut(&recipient).unwrap().agent_type = Some("limited-test-provider".into());
    mux.get_tab_mut(&recipient).unwrap().session_binding_nonce = Some("pad-send-launch".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("limited-test-provider")
            .with_paste_limit_bytes(4096),
    ));

    let body = "socket payload\nend sentinel".repeat(400);
    let params = Some(serde_json::json!({ "tab_id": recipient, "text": body.clone() }));
    let value = super::dispatch(
        "tab.send_text",
        &params,
        &fixture.service.mux,
        &std::sync::Arc::new(fixture.service.clone()),
    )
    .unwrap();
    let path = std::path::PathBuf::from(value["pad_file"].as_str().unwrap());
    assert_eq!(std::fs::read(path).unwrap(), body.as_bytes());
}
