use super::SendRejection;
use crate::service::delivery_error::DeliveryError;
use crate::service::inject_verify::InjectionError;
use crate::service::pty_io::{execute_ordered_actions, OrderedPtyAction};
use axum::http::StatusCode;

#[test]
fn owner_keys_into_a_provider_dialog_are_written_raw() {
    let keys = super::owner_dialog_keys(&[
        OrderedPtyAction::Raw(b"down".to_vec()),
        OrderedPtyAction::Submit,
    ]);
    assert_eq!(
        keys,
        vec![
            OrderedPtyAction::Raw(b"down".to_vec()),
            OrderedPtyAction::Raw(b"\r".to_vec()),
        ]
    );
}

#[test]
fn send_actions_enter_into_provider_modal_writes_zero_bytes() {
    let mut bytes_written = 0usize;
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Submit],
        |body, enter| {
            assert!(body.is_empty());
            assert!(enter);
            Err::<(), _>(DeliveryError::TabBusyRefused {
                tab: "53854ac8".into(),
                detail: "provider update dialog is visible".into(),
            })
        },
        |bytes| {
            bytes_written += bytes.len();
            Ok(())
        },
        |_| {},
    );
    let rejection = SendRejection::from(result.unwrap_err());

    assert_eq!(bytes_written, 0);
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
    assert_eq!(
        rejection.body["error"]["detail"],
        "provider update dialog is visible"
    );
}

#[test]
fn send_actions_raw_key_into_provider_modal_writes_zero_bytes() {
    let mut raw_writes = Vec::new();
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Raw(b"up".to_vec())],
        |_, _| panic!("raw action must not wait for public agent readiness"),
        |bytes| {
            if bytes == b"up" {
                Err(DeliveryError::TabBusyRefused {
                    tab: "53854ac8".into(),
                    detail: "provider trust dialog is visible".into(),
                })
            } else {
                raw_writes.push(bytes.to_vec());
                Ok(())
            }
        },
        |_| {},
    );
    let rejection = SendRejection::from(result.unwrap_err());

    assert!(raw_writes.is_empty());
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
    assert_eq!(
        rejection.body["error"]["detail"],
        "provider trust dialog is visible"
    );
}

#[test]
fn api_send_actions_text_enter_uses_acknowledged_submit() {
    let mut submits = Vec::new();
    let mut raw_writes = Vec::new();
    let result = execute_ordered_actions(
        "parked body",
        &[OrderedPtyAction::Submit],
        |text, enter| {
            submits.push((text.to_string(), enter));
            Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                waited: std::time::Duration::ZERO,
                screen_tail: "> parked body".into(),
            })
        },
        |bytes| {
            raw_writes.push(bytes.to_vec());
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(
        result,
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
    assert_eq!(submits, [("parked body".into(), true)]);
    assert!(raw_writes.is_empty());
}

#[test]
fn api_send_actions_raw_keys_ordered_bounded() {
    let actions = [
        OrderedPtyAction::Raw(b"up".to_vec()),
        OrderedPtyAction::Delay(std::time::Duration::from_millis(25)),
        OrderedPtyAction::Raw(b"esc".to_vec()),
    ];
    let mut raw_writes = Vec::new();
    let mut deadlines = Vec::new();
    let mut preflights = Vec::new();
    execute_ordered_actions(
        "",
        &actions,
        |text, enter| {
            preflights.push((text.to_string(), enter));
            Ok::<_, String>(())
        },
        |bytes| {
            raw_writes.push(bytes.to_vec());
            Ok(())
        },
        |deadline| deadlines.push(deadline),
    )
    .unwrap();
    assert_eq!(raw_writes, [b"up".to_vec(), b"esc".to_vec()]);
    assert_eq!(deadlines, [std::time::Duration::from_millis(25)]);
    assert!(preflights.is_empty());
}

#[test]
fn send_actions_enter_and_ctrl_c_to_shell_write_their_bytes() {
    let written = std::cell::RefCell::new(Vec::new());
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Submit, OrderedPtyAction::Raw(vec![0x03])],
        |text, enter| {
            assert!(text.is_empty());
            assert!(enter);
            written.borrow_mut().extend_from_slice(b"\r");
            Ok::<_, DeliveryError>(())
        },
        |bytes| {
            written.borrow_mut().extend_from_slice(bytes);
            Ok(())
        },
        |_| {},
    );

    result.unwrap();
    assert_eq!(written.into_inner(), b"\r\x03");
}

#[derive(Clone, Copy)]
enum ProviderDialogFixture {
    Trust,
    Update,
}

fn service_with_provider_dialog(
    dialog: ProviderDialogFixture,
) -> (crate::service::test_fixture::ServiceFixture, String) {
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use codeterm_plugin::tab_state::{TabState, TabStateClassification};

    const PROVIDER_ID: &str = "send-dialog-fixture";
    let pty_id = "pty-send-dialog-fixture";
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, pty_id.into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(PROVIDER_ID.into());
    tab.agent_session_id = Some("send-dialog-session".into());
    tab.session_binding_nonce = Some("send-dialog-nonce".into());
    tab.reported_session_binding_nonce = Some("send-dialog-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    let (provider, screen) = match dialog {
        ProviderDialogFixture::Trust => (
            MockProvider::with_directory_trust_response(
                PROVIDER_ID,
                codeterm_plugin::provider::DirectoryTrustPromptResponse::Hold,
            )
            .with_tab_state(TabStateClassification {
                state: TabState::WaitingOnAuth,
                confidence: 1.0,
                data: None,
                action: None,
            }),
            include_str!("../../../../../plugins/provider-codex/tests/fixtures/directory-trust/trust-folder-access-0.157.1.txt"),
        ),
        ProviderDialogFixture::Update => (
            MockProvider::new(PROVIDER_ID).with_tab_state(TabStateClassification {
                state: TabState::UpdatePrompt,
                confidence: 1.0,
                data: None,
                action: None,
            }),
            include_str!("../../../../../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt"),
        ),
    };
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, screen.as_bytes());
    (fixture, tab_id)
}

fn assert_plain_text_is_held_until_provider_dialog_clears(dialog: ProviderDialogFixture) {
    use crate::service::chat_send_transaction::ChatSendState;
    use crate::service::delivery_error::DeliveryError;
    use crate::service::parked_user_delivery::tests::{
        attach_fixture_pty, await_owner_stopped, UserSubmitBoundary,
    };
    use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    let (fixture, tab_id) = service_with_provider_dialog(dialog);
    let pty_id = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    let initial_snapshot = TabStateSnapshot {
        tab_id: tab_id.clone(),
        agent_type: "send-dialog-fixture".into(),
        session_id: Some("send-dialog-session".into()),
        cwd: None,
        screen_text: fixture
            .service
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default(),
    };
    assert!(fixture
        .service
        .agent_registry
        .with_provider("send-dialog-fixture", |provider| {
            provider.delivery_modal_visible(&initial_snapshot)
        })
        .unwrap_or(false));

    let pty_generation = attach_fixture_pty(&fixture, &tab_id);
    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id).unwrap().boot_stage =
            Some(codeterm_types::pty::BootStage::AgentReady);
    }
    let authority = fixture.service.user_send_authority(&tab_id).unwrap();
    assert!(fixture.service.latch_user_authority_ready(&authority));

    let writes = Arc::new(AtomicUsize::new(0));
    let writes_seen = writes.clone();
    let service = fixture.service.clone();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let _submit = UserSubmitBoundary::install(
        &tab_id,
        Arc::new(move |record, _| {
            let snapshot = TabStateSnapshot {
                tab_id: record.authority.tab_id.clone(),
                agent_type: record.authority.provider_id.clone(),
                session_id: record.authority.session_id.clone(),
                cwd: None,
                screen_text: service
                    .screen_state
                    .lock()
                    .get_screen_text(record.authority.pty_id.as_deref().unwrap())
                    .unwrap_or_default(),
            };
            let modal = service
                .agent_registry
                .with_provider(&record.authority.provider_id, |provider| {
                    provider.delivery_modal_visible(&snapshot)
                })
                .unwrap_or(false);
            if modal {
                let _ = attempt_tx.send(true);
                Err(DeliveryError::TabBusyRefused {
                    tab: record.authority.tab_id.clone(),
                    detail: "provider-classified modal dialog is visible".into(),
                })
            } else {
                writes_seen.fetch_add(1, Ordering::AcqRel);
                let _ = attempt_tx.send(false);
                Ok(())
            }
        }),
    );

    let accepted = fixture
        .service
        .start_chat_send(&tab_id, "follow-up message".into(), None, None)
        .unwrap();
    assert_eq!(accepted.state, ChatSendState::Pending);
    assert!(attempt_rx.recv_timeout(Duration::from_secs(10)).unwrap());
    await_owner_stopped(&fixture, &tab_id);
    assert_eq!(writes.load(Ordering::Acquire), 0);
    let held = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab_id)
        .unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .original(&tab_id, &accepted.send_id)
            .unwrap(),
        "follow-up message"
    );

    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("send-dialog-fixture").with_tab_state(
            TabStateClassification {
                state: TabState::Working,
                confidence: 1.0,
                data: None,
                action: None,
            },
        ),
    ));
    fixture.service.feed_pty_output_for_generation(
        &pty_id,
        pty_generation,
        b"\x1b[2J\x1b[HComposer ready\r\n",
    );
    assert!(!attempt_rx.recv_timeout(Duration::from_secs(10)).unwrap());
    await_owner_stopped(&fixture, &tab_id);
    assert_eq!(writes.load(Ordering::Acquire), 1);
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab_id)
        .unwrap()
        .is_empty());
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .original(&tab_id, &accepted.send_id)
        .is_ok());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plain_text_is_held_when_a_trust_dialog_frame_is_on_screen() {
    assert_plain_text_is_held_until_provider_dialog_clears(ProviderDialogFixture::Trust);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plain_text_is_held_then_written_once_when_an_update_dialog_clears() {
    assert_plain_text_is_held_until_provider_dialog_clears(ProviderDialogFixture::Update);
}

#[test]
fn a_keystroke_only_send_carries_no_return_address() {
    use super::prefixes_return_address;
    assert!(!prefixes_return_address("", true));
    assert!(prefixes_return_address("answer the prompt", true));
    assert!(!prefixes_return_address("answer the prompt", false));
}
