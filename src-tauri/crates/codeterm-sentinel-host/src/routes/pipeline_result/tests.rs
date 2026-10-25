use super::*;
use codeterm_events::EventBroadcaster;
use codeterm_service_core::multiplexer::{state::MultiplexerState, types::TabBacking};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::cell::Cell;

#[test]
fn blueprint_result_shell_write_and_chat_acceptance_confirm_without_turn_evidence() {
    let (mut mux, shell) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&shell).unwrap();
    let chat = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&chat).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "opaque-plugin".into(),
        session_id: "own-session".into(),
    };
    let fixture = ServiceFixture::new(mux);
    assert_eq!(
        result_delivery_class(&fixture.service, &shell),
        Ok(DeliveryClass::AgentTraffic)
    );
    assert_eq!(
        result_delivery_class(&fixture.service, &chat),
        Ok(DeliveryClass::SystemNotice)
    );
    for (target, receipt) in [
        (
            shell,
            Delivery::Pty {
                text: "opaque packet".into(),
            },
        ),
        (
            chat,
            Delivery::ChatBackend {
                message_id: "own-message".into(),
            },
        ),
    ] {
        let attempts = Cell::new(0);
        let result = deliver_pipeline_result_with(
            &target,
            "opaque packet",
            "inbox/result.md".into(),
            |_, _| {
                attempts.set(attempts.get() + 1);
                codeterm_service_core::async_runtime::block_on(confirm_result_receipt(
                    receipt,
                    &mut fixture.service.events.subscribe(),
                    &target,
                    Duration::ZERO,
                    |_| panic!("transport acceptance needs no turn evidence"),
                ))
            },
        );
        assert_eq!(result.state, "delivered");
        assert_eq!(attempts.get(), 1);
    }
}

#[tokio::test]
async fn blueprint_result_pending_late_write_confirms_exact_delivery_without_replay() {
    let bus = EventBroadcaster::new();
    let mut events = bus.subscribe();
    let attempts = Cell::new(0);
    let waiting = confirm_result_receipt(
        Delivery::Pending {
            delivery_id: "own-delivery".into(),
        },
        &mut events,
        "target",
        Duration::from_secs(60),
        |_| None,
    );
    let publish = async {
        attempts.set(attempts.get() + 1);
        bus.publish_internal(
            "delivery-written",
            serde_json::json!({"tab_id":"other", "delivery_id":"own-delivery"}),
        );
        bus.publish_internal(
            "delivery-written",
            serde_json::json!({"tab_id":"target", "delivery_id":"other"}),
        );
        bus.publish_internal(
            "delivery-written",
            serde_json::json!({"tab_id":"target", "delivery_id":"own-delivery"}),
        );
    };
    let (result, ()) = tokio::join!(waiting, publish);
    assert_eq!(result, Ok(()));
    assert_eq!(attempts.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn blueprint_result_wrong_receipts_and_inbox_presence_never_confirm() {
    let bus = EventBroadcaster::new();
    let mut events = bus.subscribe();
    bus.publish_internal(
        "delivery-written",
        serde_json::json!({"tab_id":"other", "delivery_id":"own"}),
    );
    bus.publish_internal(
        "delivery-written",
        serde_json::json!({"tab_id":"target", "delivery_id":"other"}),
    );
    bus.publish_internal(
        "inbox-written",
        serde_json::json!({"tab_id":"target", "delivery_id":"own"}),
    );
    let result = confirm_result_receipt(
        Delivery::Unconfirmed {
            delivery_id: "own".into(),
        },
        &mut events,
        "target",
        Duration::from_secs(60),
        |_| None,
    )
    .await;
    assert_eq!(
        result.as_ref().map_err(String::as_str),
        Err("result_delivery_deadline")
    );
    let receipt =
        deliver_pipeline_result_with("target", "opaque", "inbox/result.md".into(), |_, _| result);
    assert_eq!(receipt.state, "failed");
    assert_eq!(receipt.reason, "result_delivery_deadline");
}

#[tokio::test]
async fn blueprint_result_closed_or_failed_target_finishes_with_typed_failure() {
    for close in [true, false] {
        let bus = EventBroadcaster::new();
        let mut events = bus.subscribe();
        if close {
            bus.publish(SystemEvent::TabClosed {
                tab_id: "target".into(),
            });
        } else {
            bus.publish_internal(
                "agent-delivery-status",
                serde_json::json!({"tab_id":"target", "delivery_id":"own", "status":"failed"}),
            );
        }
        let result = confirm_result_receipt(
            Delivery::Pending {
                delivery_id: "own".into(),
            },
            &mut events,
            "target",
            Duration::from_secs(60),
            |_| None,
        )
        .await;
        assert_eq!(
            result.as_ref().map_err(String::as_str),
            Err("result_target_unavailable")
        );
    }
}

#[tokio::test]
async fn blueprint_result_late_runtime_acceptance_resolves_unconfirmed_receipt() {
    let bus = EventBroadcaster::new();
    let mut events = bus.subscribe();
    let accepted = Cell::new(false);
    let waiting = confirm_result_receipt(
        Delivery::Unconfirmed {
            delivery_id: "own".into(),
        },
        &mut events,
        "target",
        Duration::from_secs(60),
        |id| {
            assert_eq!(id, "own");
            accepted.get().then_some(Ok(()))
        },
    );
    let accept = async {
        accepted.set(true);
        bus.publish_internal(
            "chat-send-status",
            serde_json::json!({"tab_id":"target", "send_id":"own", "state":"delivered"}),
        );
    };
    let (result, ()) = tokio::join!(waiting, accept);
    assert_eq!(result, Ok(()));
}

#[test]
fn blueprint_result_missing_or_failed_launch_is_refused_before_delivery() {
    let (mut mux, target) = MultiplexerState::new();
    mux.get_tab_mut(&target).unwrap().boot_stage =
        Some(codeterm_types::pty::BootStage::SpawnFailed);
    let fixture = ServiceFixture::new(mux);
    assert_eq!(
        result_delivery_class(&fixture.service, "missing"),
        Err("result_target_unavailable")
    );
    assert_eq!(
        result_delivery_class(&fixture.service, &target),
        Err("result_target_unavailable")
    );
}
