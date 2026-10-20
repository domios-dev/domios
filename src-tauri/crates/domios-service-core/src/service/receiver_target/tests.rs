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
    service
        .ext::<codeterm_plugin::WebhookRegistryHandle>()
        .register(RECEIVER, receiver);
    service
        .ext::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>()
        .register(
            RECEIVER,
            Arc::new(vec![TabMenuItem {
                id: ITEM.into(),
                label: LABEL.into(),
                action: TabMenuAction::ReceiverTarget,
                icon: None,
                icon_svg: Some(
                    "<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>"
                        .into(),
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
    assert!(service
        .ext::<crate::service::receiver_target::ReceiverTargets>()
        .0
        .bindings()
        .is_empty());
}

#[test]
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

    service
        .ext::<crate::service::receiver_target::ReceiverTargets>()
        .0
        .set(RECEIVER, "tab-gone")
        .unwrap();
    let stale = service.receiver_target(RECEIVER).unwrap();
    assert_eq!(stale.tab_id.as_deref(), Some("tab-gone"));
    assert!(!stale.live);
    assert_eq!(service.live_receiver_target(RECEIVER), None);

    service.set_receiver_target(RECEIVER, &scene.tab).unwrap();
    service.forget_receiver_targets_for_tab(&scene.other);
    assert_eq!(
        service
            .ext::<crate::service::receiver_target::ReceiverTargets>()
            .0
            .get(RECEIVER),
        Some(scene.tab.clone())
    );
    service.forget_receiver_targets_for_tab(&scene.tab);
    assert_eq!(
        service
            .ext::<crate::service::receiver_target::ReceiverTargets>()
            .0
            .get(RECEIVER),
        None
    );
    assert_eq!(service.live_receiver_target(RECEIVER), None);
}
