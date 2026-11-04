use super::*;
use codeterm_browser::{
    cdp::{CdpClient, CdpConnection, CdpError, EventStream},
    cdp_wire::{CdpEvent, EventFilter},
};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch, Notify};

#[derive(Clone, Copy)]
enum Effect {
    Control,
    SameDocument,
    Navigation,
    HeldNavigation,
    Close,
}

struct Connection {
    subscribers: Mutex<Vec<(EventFilter, mpsc::Sender<Arc<CdpEvent>>)>>,
    closed: watch::Sender<Option<String>>,
    effect: Effect,
    actions: AtomicUsize,
    touch: AtomicBool,
    poisoned: AtomicBool,
    hold_restore: AtomicBool,
    restore_started: Notify,
    restore_release: Notify,
    fail_mouse: AtomicBool,
    input_commands: Mutex<Vec<(String, Value)>>,
}

impl Connection {
    fn emit(&self, method: &str, params: Value) {
        let event = Arc::new(CdpEvent {
            method: method.into(),
            params,
            session_id: Some("session".into()),
        });
        for (filter, sender) in self.subscribers.lock().iter() {
            if filter.matches(&event) {
                let _ = sender.try_send(event.clone());
            }
        }
    }

    fn act(&self) {
        self.actions.fetch_add(1, Ordering::SeqCst);
        match self.effect {
            Effect::Control => {}
            Effect::SameDocument => self.emit(
                "Page.navigatedWithinDocument",
                json!({"frameId":"main","url":"https://page/#changed"}),
            ),
            Effect::Navigation => {
                self.emit("Page.frameRequestedNavigation", json!({"frameId":"main"}))
            }
            Effect::HeldNavigation => {
                self.emit("Page.frameStartedLoading", json!({"frameId":"main"}))
            }
            Effect::Close => self.emit("Inspector.detached", json!({"reason":"target_closed"})),
        }
    }

    fn release_navigation(&self) {
        self.emit(
            "Page.frameNavigated",
            json!({"frame":{"id":"main","loaderId":"after","url":"https://page/delayed"}}),
        );
        self.emit(
            "Page.lifecycleEvent",
            json!({"frameId":"main","loaderId":"after","name":"load"}),
        );
    }
}

#[async_trait::async_trait]
impl CdpConnection for Connection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/completion"
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
    }
    fn subscribe(&self, filter: EventFilter) -> EventStream {
        let (sender, receiver) = mpsc::channel(64);
        self.subscribers.lock().push((filter, sender));
        EventStream::from_receiver(receiver)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        if method.starts_with("Input.") || method == "Emulation.setEmitTouchEventsForMouse" {
            self.input_commands
                .lock()
                .push((method.into(), params.clone()));
        }
        if method == "Input.dispatchMouseEvent"
            && (self.touch.load(Ordering::SeqCst) || self.poisoned.load(Ordering::SeqCst))
            && matches!(
                params["type"].as_str(),
                Some("mousePressed" | "mouseReleased")
            )
        {
            self.poisoned.store(true, Ordering::SeqCst);
            if params["type"] == "mouseReleased" {
                self.act();
            }
            return std::future::pending().await;
        }
        if method == "Input.dispatchMouseEvent" && self.fail_mouse.load(Ordering::SeqCst) {
            return Err(CdpError::Malformed("injected mouse failure".into()));
        }
        Ok(match method {
            "Emulation.setEmitTouchEventsForMouse" => {
                self.touch
                    .store(params["enabled"] == true, Ordering::SeqCst);
                if params["enabled"] == true && self.hold_restore.load(Ordering::SeqCst) {
                    self.restore_started.notify_one();
                    self.restore_release.notified().await;
                }
                json!({})
            }
            "Input.synthesizeTapGesture" => {
                self.act();
                json!({})
            }
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}}),
            "DOM.getContentQuads" => json!({"quads":[[0,0,20,0,20,20,0,20]]}),
            "DOM.resolveNode" => json!({"object":{"objectId":"button"}}),
            "DOM.getDocument" => json!({"root":{}}),
            "Accessibility.getFullAXTree" => json!({"nodes":[
                {"nodeId":"form","role":{"value":"form"},"name":{"value":"Composer"},"childIds":["button"]},
                {"nodeId":"button","parentId":"form","role":{"value":"button"},"name":{"value":"Go"},"backendDOMNodeId":7}
            ]}),
            "Runtime.evaluate" => json!({"result":{"value": if params["expression"].as_str()
                .is_some_and(|source|source.contains("state.id === capture.id")) {json!(true)} else {json!(0)}}}),
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|source| source.contains("function domiosVisible")) =>
            {
                json!({"result":{"value":true}})
            }
            "Runtime.callFunctionOn" => {
                if params["awaitPromise"] == true {
                    return Ok(json!({"result":{}}));
                }
                self.act();
                json!({"result":{"value":"performed"}})
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                self.act();
                json!({})
            }
            "Page.getNavigationHistory" => {
                json!({"currentIndex":1,"entries":[{"id":1},{"id":2},{"id":3}]})
            }
            "Page.navigate" | "Page.navigateToHistoryEntry" | "Page.reload" => {
                self.act();
                if method == "Page.navigate" && matches!(self.effect, Effect::Navigation) {
                    json!({"loaderId":"after","frameId":"main"})
                } else {
                    json!({})
                }
            }
            _ => json!({}),
        })
    }
}

async fn fixture(
    effect: Effect,
) -> (
    BrowserRuntime,
    PageChannel,
    Arc<Connection>,
    tempfile::TempDir,
) {
    let connection = Arc::new(Connection {
        subscribers: Mutex::new(Vec::new()),
        closed: watch::channel(None).0,
        effect,
        actions: AtomicUsize::new(0),
        touch: AtomicBool::new(false),
        poisoned: AtomicBool::new(false),
        hold_restore: AtomicBool::new(false),
        restore_started: Notify::new(),
        restore_release: Notify::new(),
        fail_mouse: AtomicBool::new(false),
        input_commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("target")
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    runtime.shared.bind(&page);
    (runtime, page, connection, root)
}

async fn device_mode(runtime: &BrowserRuntime, page: &PageChannel, mobile: bool) {
    runtime
        .host
        .set_device_viewport(
            codeterm_browser::viewport::ViewportRequest {
                width: 380.0,
                height: 844.0,
                dpr: 2.0,
            },
            mobile,
        )
        .await
        .unwrap();
    page.send(
        "Emulation.setEmitTouchEventsForMouse",
        json!({"enabled":mobile}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_completion_mobile_tap_then_desktop_click_confirm_without_replay() {
    let (runtime, page, connection, _root) = fixture(Effect::Control).await;
    for mobile in [false, true, false] {
        device_mode(&runtime, &page, mobile).await;
        connection.input_commands.lock().clear();
        let before = connection.actions.load(Ordering::SeqCst);
        let action = runtime.click_node(&page, 7);
        tokio::pin!(action);
        let result = match futures_util::poll!(&mut action) {
            std::task::Poll::Ready(result) => result,
            std::task::Poll::Pending => {
                panic!("delivered click did not confirm synchronously; mobile={mobile}")
            }
        };
        assert_eq!(
            result.unwrap(),
            "action acknowledged; asynchronous effects require an explicit state wait"
        );
        assert_eq!(connection.actions.load(Ordering::SeqCst), before + 1);
        let commands = connection.input_commands.lock();
        if mobile {
            assert_eq!(
                *commands,
                vec![(
                    "Input.synthesizeTapGesture".into(),
                    json!({
                        "x":10.0,"y":10.0,"gestureSourceType":"mouse","tapCount":1,"duration":0,
                    })
                )]
            );
        } else {
            assert_eq!(
                *commands,
                ["mouseMoved", "mousePressed", "mouseReleased"]
                    .map(|kind| (
                        "Input.dispatchMouseEvent".to_owned(),
                        json!({"type":kind,"x":10.0,"y":10.0,"button":"left","clickCount":1})
                    ))
                    .to_vec()
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn browser_completion_mobile_mouse_interactions_and_viewer_input_do_not_poison_clicks() {
    let (runtime, page, connection, _root) = fixture(Effect::Control).await;
    device_mode(&runtime, &page, true).await;
    for verb in ["right-click", "double-click", "triple-click", "hover"] {
        runtime.snapshot_verb(&page, &[]).await.unwrap();
        connection.input_commands.lock().clear();
        runtime
            .interaction_verb(&page, verb, &["button".into(), "Go".into()])
            .await
            .unwrap();
        let commands = connection.input_commands.lock().clone();
        assert_eq!(
            commands.first().unwrap(),
            &(
                "Emulation.setEmitTouchEventsForMouse".into(),
                json!({"enabled":false})
            )
        );
        assert_eq!(
            commands.last().unwrap(),
            &(
                "Emulation.setEmitTouchEventsForMouse".into(),
                json!({"enabled":true,"configuration":"mobile"})
            )
        );
        assert!(commands[1..commands.len() - 1]
            .iter()
            .all(|(method, _)| method == "Input.dispatchMouseEvent"));
        assert!(connection.touch.load(Ordering::SeqCst));
    }
    for (phase, buttons) in [("down", 1), ("move", 1), ("up", 0)] {
        runtime
            .viewer_input(
                &page,
                &super::super::input::InputArgs {
                    kind: "pointer".into(),
                    phase: Some(phase.into()),
                    buttons: Some(buttons),
                    x: Some(10.0),
                    y: Some(10.0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(connection.touch.load(Ordering::SeqCst));
    }
    device_mode(&runtime, &page, false).await;
    assert!(runtime
        .click_node(&page, 7)
        .await
        .unwrap()
        .starts_with("action acknowledged;"));
    device_mode(&runtime, &page, true).await;
    assert!(runtime
        .click_node(&page, 7)
        .await
        .unwrap()
        .starts_with("action acknowledged;"));
    assert!(!connection.poisoned.load(Ordering::SeqCst));
}

#[tokio::test]
async fn browser_completion_cancelled_mouse_scope_holds_gate_until_touch_restore_ack() {
    let (runtime, page, connection, _root) = fixture(Effect::Control).await;
    device_mode(&runtime, &page, true).await;
    let scope = runtime.action_scope(&page, true).await.unwrap();
    connection.hold_restore.store(true, Ordering::SeqCst);
    drop(scope);
    connection.restore_started.notified().await;
    assert!(runtime.action_gate.try_lock().is_err());
    connection.restore_release.notify_one();
    let next = runtime.action_scope(&page, false).await.unwrap();
    assert!(connection.touch.load(Ordering::SeqCst));
    next.finish().await.unwrap();
    connection.hold_restore.store(false, Ordering::SeqCst);
    connection.fail_mouse.store(true, Ordering::SeqCst);
    assert!(runtime
        .viewer_input(&page, &super::super::input::InputArgs::click(10.0, 10.0))
        .await
        .unwrap_err()
        .contains("injected mouse failure"));
    assert!(connection.touch.load(Ordering::SeqCst));
    assert!(runtime.action_gate.try_lock().is_ok());
}

async fn click(
    runtime: &BrowserRuntime,
    page: &PageChannel,
    atomic: bool,
) -> Result<String, String> {
    if atomic {
        runtime
            .atomic_action(
                page,
                &[
                    "--within",
                    "form:Composer",
                    "--role",
                    "button",
                    "--name",
                    "Go",
                ]
                .map(String::from),
                false,
            )
            .await
    } else {
        runtime.click_node(page, 7).await
    }
}

#[tokio::test]
async fn browser_completion_direct_and_atomic_clicks_wait_for_released_navigation() {
    for (atomic, mobile) in [(false, false), (true, false), (false, true)] {
        for effect in [Effect::Navigation, Effect::HeldNavigation] {
            let (runtime, page, connection, _root) = fixture(effect).await;
            device_mode(&runtime, &page, mobile).await;
            let action = click(&runtime, &page, atomic);
            tokio::pin!(action);
            assert!(futures_util::poll!(&mut action).is_pending());
            assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
            connection.emit(
                "Page.lifecycleEvent",
                json!({"frameId":"child","loaderId":"after","name":"load"}),
            );
            connection.emit(
                "Page.lifecycleEvent",
                json!({"frameId":"main","loaderId":"before","name":"load"}),
            );
            assert!(futures_util::poll!(&mut action).is_pending());
            connection.release_navigation();
            assert!(action
                .await
                .unwrap()
                .starts_with("navigation loaded: https://page/delayed"));
            assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn browser_completion_direct_and_atomic_control_same_document_and_close_receipts() {
    for atomic in [false, true] {
        for (effect, receipt) in [
            (Effect::Control, "action acknowledged;"),
            (
                Effect::SameDocument,
                "same-document navigation: https://page/#changed",
            ),
            (Effect::Close, "action_unconfirmed: page closed;"),
        ] {
            let (runtime, page, connection, _root) = fixture(effect).await;
            let result = click(&runtime, &page, atomic).await;
            let actual = result.unwrap_or_else(|error| error);
            assert!(actual.starts_with(receipt), "{actual}");
            assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn browser_completion_goto_and_history_need_their_own_navigation() {
    use crate::surface::browser::input::NavAction;
    for nav in [
        NavAction::Goto,
        NavAction::Back,
        NavAction::Forward,
        NavAction::Reload,
    ] {
        let (runtime, page, connection, _root) = fixture(Effect::Navigation).await;
        let action = runtime.navigate(&page, nav, Some("https://page/delayed"));
        tokio::pin!(action);
        assert!(futures_util::poll!(&mut action).is_pending());
        connection.release_navigation();
        assert_eq!(
            action.await.unwrap(),
            "navigation loaded: https://page/delayed"
        );
        assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn browser_completion_timeout_and_changed_authority_never_replay() {
    for atomic in [false, true] {
        let (runtime, page, connection, _root) = fixture(Effect::HeldNavigation).await;
        let result = click(&runtime, &page, atomic).await;
        assert!(result.unwrap_err().contains("deadline exceeded"));
        assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
        let action = click(&runtime, &page, atomic);
        tokio::pin!(action);
        assert!(futures_util::poll!(&mut action).is_pending());
        runtime.shared.invalidate_action();
        connection.release_navigation();
        assert!(action.await.unwrap_err().contains("authority changed"));
        assert_eq!(connection.actions.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn browser_completion_rejects_prior_connection_with_reused_session_id() {
    let (runtime, page, connection, _root) = fixture(Effect::Navigation).await;
    let (_other_runtime, replacement, _other_connection, _other_root) =
        fixture(Effect::Control).await;
    let action = click(&runtime, &page, false);
    tokio::pin!(action);
    assert!(futures_util::poll!(&mut action).is_pending());
    runtime.shared.bind(&replacement);
    connection.release_navigation();
    assert!(action.await.unwrap_err().contains("authority changed"));
    assert!(runtime.shared.begin_action(&page).is_err());
}
