use super::*;
use codeterm_browser::{
    cdp::{CdpClient, CdpConnection, CdpError, EventStream},
    cdp_wire::{CdpEvent, EventFilter},
};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

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
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}}),
            "DOM.getContentQuads" => json!({"quads":[[0,0,20,0,20,20,0,20]]}),
            "DOM.resolveNode" => json!({"object":{"objectId":"button"}}),
            "Accessibility.getFullAXTree" => json!({"nodes":[
                {"nodeId":"form","role":{"value":"form"},"name":{"value":"Composer"},"childIds":["button"]},
                {"nodeId":"button","parentId":"form","role":{"value":"button"},"name":{"value":"Go"},"backendDOMNodeId":7}
            ]}),
            "Runtime.evaluate" => json!({"result":{"value":0}}),
            "Runtime.callFunctionOn" => {
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
    for atomic in [false, true] {
        for effect in [Effect::Navigation, Effect::HeldNavigation] {
            let (runtime, page, connection, _root) = fixture(effect).await;
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
