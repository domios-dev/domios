use std::sync::Arc;

use super::*;
use crate::plugin::tab_menu::TabMenuItem;
use crate::plugin::{
    WebhookDelivery, WebhookReceiveContext, WebhookReceiveError, WebhookReceiver,
    WebhookReceiverKind,
};
use crate::service::test_fixture::ServiceFixture;

pub(super) const RECEIVER: &str = "receiver";
const ITEM: &str = "receiver-target";
const LABEL: &str = "Receive events here";

fn store_at(dir: &tempfile::TempDir) -> ReceiverTargetStore {
    ReceiverTargetStore::open(dir.path().join("receiver-targets.json"))
}

#[test]
fn pebble_receiver_target_store_set_clear_get() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(&dir);
    assert_eq!(store.get(RECEIVER), None);

    assert!(store.set(RECEIVER, "tab-a").unwrap());
    assert!(
        !store.set(RECEIVER, "tab-a").unwrap(),
        "rebinding the same tab is not a change"
    );
    assert_eq!(store.get(RECEIVER).as_deref(), Some("tab-a"));

    assert!(store.set(RECEIVER, "tab-b").unwrap());
    assert_eq!(store.get(RECEIVER).as_deref(), Some("tab-b"));
    assert_eq!(store.bindings().len(), 1, "one tab per receiver");

    assert!(store.clear(RECEIVER).unwrap());
    assert!(!store.clear(RECEIVER).unwrap());
    assert_eq!(store.get(RECEIVER), None);
}

#[test]
fn pebble_receiver_target_store_round_trips_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = store_at(&dir);
        store.set(RECEIVER, "tab-a").unwrap();
        store.set("second-receiver", "tab-b").unwrap();
    }
    let reopened = store_at(&dir);
    assert_eq!(reopened.get(RECEIVER).as_deref(), Some("tab-a"));
    assert_eq!(reopened.get("second-receiver").as_deref(), Some("tab-b"));

    reopened.clear(RECEIVER).unwrap();
    drop(reopened);
    let after_reset = store_at(&dir);
    assert_eq!(after_reset.get(RECEIVER), None);
    assert_eq!(after_reset.get("second-receiver").as_deref(), Some("tab-b"));
}

#[test]
fn pebble_receiver_target_store_forget_tab_clears_only_that_tab() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_at(&dir);
    store.set("a", "tab-1").unwrap();
    store.set("b", "tab-1").unwrap();
    store.set("c", "tab-2").unwrap();

    assert!(store.forget_tab("tab-1").unwrap());
    assert!(!store.forget_tab("tab-1").unwrap());
    assert_eq!(store.get("a"), None);
    assert_eq!(store.get("b"), None);
    assert_eq!(store.get("c").as_deref(), Some("tab-2"));
    assert_eq!(store_at(&dir).bindings().len(), 1);
}

#[test]
fn pebble_receiver_target_store_unreadable_file_opens_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("receiver-targets.json"), "{").unwrap();
    let store = store_at(&dir);
    assert!(store.bindings().is_empty());
    assert!(store.set(RECEIVER, "tab-a").unwrap());
    assert_eq!(store_at(&dir).get(RECEIVER).as_deref(), Some("tab-a"));
}

#[test]
fn pebble_receiver_target_failed_write_keeps_the_previous_binding() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiverTargetStore::open(dir.path().join("missing").join("receiver-targets.json"));
    assert!(store.set(RECEIVER, "tab-a").is_err());
    assert_eq!(
        store.get(RECEIVER),
        None,
        "memory never claims a binding a restart would lose"
    );
}

#[test]
fn pebble_receiver_target_routes_only_to_a_live_bound_tab() {
    assert_eq!(
        live_target(None, |_| unreachable!("an unbound receiver checks no tab")),
        None
    );
    for (liveness, expected) in [
        (TargetLiveness::Live, Some("tab-a".to_string())),
        (TargetLiveness::NotLive, None),
        (TargetLiveness::Absent, None),
    ] {
        let routed = live_target(Some("tab-a".into()), |tab_id| {
            assert_eq!(tab_id, "tab-a");
            liveness
        });
        assert_eq!(routed, expected, "{liveness:?}");
    }
}

#[test]
fn pebble_receiver_target_host_json_carries_codes_not_details() {
    let view: serde_json::Value = serde_json::from_str(&host_json(Ok(ReceiverTargetView {
        plugin_id: RECEIVER.into(),
        tab_id: Some("tab-a".into()),
        live: true,
    })))
    .unwrap();
    assert_eq!(
        view,
        serde_json::json!({ "pluginId": RECEIVER, "tabId": "tab-a", "live": true })
    );

    let refused: serde_json::Value =
        serde_json::from_str(&host_json(Err(ReceiverTargetError::PersistFailed))).unwrap();
    assert_eq!(refused["error"], "persist_failed");
    assert_eq!(refused.as_object().unwrap().len(), 2);
}

struct SilentReceiver;

impl WebhookReceiver for SilentReceiver {
    fn id(&self) -> &str {
        RECEIVER
    }

    fn kind(&self) -> WebhookReceiverKind {
        WebhookReceiverKind::Pebble
    }

    fn receive(
        &self,
        _context: WebhookReceiveContext,
    ) -> Result<Option<WebhookDelivery>, WebhookReceiveError> {
        Ok(None)
    }
}

pub(super) struct Scene {
    pub(super) fixture: ServiceFixture,
    pub(super) tab: String,
    pub(super) other: String,
}

pub(super) fn scene() -> Scene {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let receiver: Arc<dyn WebhookReceiver> = Arc::new(SilentReceiver);
    service.webhook_registry.register(RECEIVER, receiver);
    service.tab_menu_registry.register(
        RECEIVER,
        Arc::new(vec![TabMenuItem {
            id: ITEM.into(),
            label: LABEL.into(),
            action: TabMenuAction::ReceiverTarget,
            icon: None,
            icon_svg: Some(
                "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>".into(),
            ),
            icon_color: Some("#d97757".into()),
        }]),
    );
    let workspace = service
        .mux
        .write()
        .create_workspace("Receivers".into(), None)
        .id;
    let tab = service.create_tab(&workspace).unwrap();
    let other = service.create_tab(&workspace).unwrap();
    Scene {
        fixture,
        tab,
        other,
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pebble_receiver_target_service_refuses_foreign_plugins_tabs_and_items() {
    let scene = scene();
    let service = &scene.fixture.service;

    assert_eq!(
        service.set_receiver_target("other-plugin", &scene.tab),
        Err(ReceiverTargetError::ReceiverNotFound)
    );
    assert_eq!(
        service.receiver_target("other-plugin"),
        Err(ReceiverTargetError::ReceiverNotFound)
    );
    assert_eq!(
        service.set_receiver_target(RECEIVER, "tab-on-a-peer"),
        Err(ReceiverTargetError::TabNotFound)
    );
    assert_eq!(
        service.run_tab_menu_item(RECEIVER, "close-tab", &scene.tab),
        Err(ReceiverTargetError::MenuItemNotFound)
    );
    assert_eq!(
        service.run_tab_menu_item("other-plugin", ITEM, &scene.tab),
        Err(ReceiverTargetError::MenuItemNotFound)
    );
    assert_eq!(
        service.run_tab_menu_item(RECEIVER, ITEM, "tab-on-a-peer"),
        Err(ReceiverTargetError::TabNotFound)
    );
    assert_eq!(service.receiver_target(RECEIVER).unwrap().tab_id, None);
    assert!(service.receiver_targets.bindings().is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plugin_icon_svg_tab_menu_payload_and_target_action_toggles_one_binding() {
    let scene = scene();
    let service = &scene.fixture.service;

    let payload = serde_json::to_value(service.tab_menu_state()).unwrap();
    assert_eq!(
        payload["items"][0]["iconSvg"],
        serde_json::json!(
            "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>"
        ),
        "the same state feeds the REST and Tauri tab-menu payloads",
    );

    let bound = service
        .run_tab_menu_item(RECEIVER, ITEM, &scene.tab)
        .unwrap();
    assert_eq!(
        bound.items,
        vec![TabMenuEntry {
            plugin_id: RECEIVER.into(),
            item_id: ITEM.into(),
            label: LABEL.into(),
            action: TabMenuAction::ReceiverTarget,
            icon_svg: Some(
                "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>".into()
            ),
            icon_color: Some("#d97757".into()),
        }]
    );
    assert_eq!(bound.receiver_targets.get(RECEIVER), Some(&scene.tab));

    let moved = service
        .run_tab_menu_item(RECEIVER, ITEM, &scene.other)
        .unwrap();
    assert_eq!(moved.receiver_targets.len(), 1);
    assert_eq!(moved.receiver_targets.get(RECEIVER), Some(&scene.other));

    let cleared = service
        .run_tab_menu_item(RECEIVER, ITEM, &scene.other)
        .unwrap();
    assert!(cleared.receiver_targets.is_empty());

    service.set_receiver_target(RECEIVER, &scene.tab).unwrap();
    let reset = service.clear_receiver_target(RECEIVER).unwrap();
    assert_eq!(reset.tab_id, None);
    assert_eq!(service.live_receiver_target(RECEIVER), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn pebble_receiver_target_restart_close_and_absent_tab_fall_back() {
    let scene = scene();
    let service = &scene.fixture.service;
    service.set_receiver_target(RECEIVER, &scene.tab).unwrap();

    let reopened =
        ReceiverTargetStore::open(scene.fixture.temp.path().join("receiver-targets.json"));
    assert_eq!(reopened.get(RECEIVER), Some(scene.tab.clone()));

    let view = service.receiver_target(RECEIVER).unwrap();
    assert_eq!(
        view.live,
        service.live_receiver_target(RECEIVER).is_some(),
        "the view and the router observe the same liveness"
    );

    service.receiver_targets.set(RECEIVER, "tab-gone").unwrap();
    let stale = service.receiver_target(RECEIVER).unwrap();
    assert_eq!(stale.tab_id.as_deref(), Some("tab-gone"));
    assert!(!stale.live);
    assert_eq!(service.live_receiver_target(RECEIVER), None);

    service.set_receiver_target(RECEIVER, &scene.tab).unwrap();
    service.forget_receiver_targets_for_tab(&scene.other);
    assert_eq!(
        service.receiver_targets.get(RECEIVER),
        Some(scene.tab.clone())
    );
    service.forget_receiver_targets_for_tab(&scene.tab);
    assert_eq!(service.receiver_targets.get(RECEIVER), None);
    assert_eq!(service.live_receiver_target(RECEIVER), None);
}
