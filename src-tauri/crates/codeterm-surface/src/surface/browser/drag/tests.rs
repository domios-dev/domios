use super::*;
use codeterm_browser::{
    cdp::{CdpClient, CdpConnection, CdpError},
    cdp_wire::{CdpEvent, EventFilter},
};
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy)]
enum Mode {
    Native,
    Pointer,
    NoDrop,
    NoIntercept,
}

struct Connection {
    mode: Mode,
    calls: Mutex<Vec<(String, Value)>>,
    subscribers: Mutex<Vec<(EventFilter, mpsc::Sender<Arc<CdpEvent>>)>>,
    closed: watch::Sender<Option<String>>,
}

fn payload() -> Value {
    json!({"items":[{"mimeType":"text/plain","data":"actual-page-payload"}],"dragOperationsMask":1})
}

#[async_trait::async_trait]
impl CdpConnection for Connection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/drag"
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
        self.calls.lock().push((method.into(), params.clone()));
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}}),
            "Input.dispatchMouseEvent"
                if params["type"] == "mouseMoved"
                    && params["buttons"] == 1
                    && params["x"].as_f64() == Some(50.0) =>
            {
                if matches!(self.mode, Mode::Native | Mode::NoDrop) {
                    let event = Arc::new(CdpEvent {
                        method: "Input.dragIntercepted".into(),
                        session_id: Some("session".into()),
                        params: json!({"data":payload()}),
                    });
                    for (filter, sender) in self.subscribers.lock().iter() {
                        if filter.matches(&event) {
                            sender.try_send(event.clone()).unwrap();
                        }
                    }
                }
                json!({})
            }
            "Runtime.evaluate" => {
                let expression = params["expression"].as_str().unwrap();
                let value = if expression.contains("domios.browser.drag") {
                    if expression.contains("\"arm\",") || expression.contains("\"clear\",") {
                        json!(true)
                    } else {
                        let calls = self.calls.lock();
                        json!({"started":!matches!(self.mode,Mode::Pointer),"dropped":!matches!(self.mode,Mode::NoDrop) && calls.iter().any(|(m,p)| m == "Input.dispatchDragEvent" && p["type"] == "drop"),"released":calls.iter().any(|(m,p)| m == "Input.dispatchMouseEvent" && p["type"] == "mouseReleased")})
                    }
                } else {
                    json!(true)
                };
                json!({"result":{"value":value}})
            }
            _ => json!({}),
        })
    }
}

async fn fixture(
    mode: Mode,
) -> (
    BrowserRuntime,
    PageChannel,
    Arc<Connection>,
    tempfile::TempDir,
) {
    let connection = Arc::new(Connection {
        mode,
        calls: Mutex::new(Vec::new()),
        subscribers: Mutex::new(Vec::new()),
        closed: watch::channel(None).0,
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

#[tokio::test]
async fn browser_native_drag_uses_intercepted_payload_in_order_and_clears_interception() {
    let (runtime, page, connection, _root) = fixture(Mode::Native).await;
    let result = runtime
        .drag_action(&page, [0.0, 0.0, 100.0, 100.0], json!({}))
        .await
        .unwrap();
    assert!(result.contains("native drop event observed"));
    let calls = connection.calls.lock();
    let drags: Vec<_> = calls
        .iter()
        .filter(|(m, _)| m == "Input.dispatchDragEvent")
        .collect();
    assert_eq!(
        drags
            .iter()
            .map(|(_, p)| p["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["dragEnter", "dragOver", "drop"]
    );
    for (_, params) in drags {
        assert_eq!(params["data"], payload());
        assert_eq!(params["x"], 100.0);
        assert_eq!(params["y"], 100.0);
    }
    let interception: Vec<_> = calls
        .iter()
        .filter(|(m, _)| m == "Input.setInterceptDrags")
        .map(|(_, p)| p["enabled"].clone())
        .collect();
    assert_eq!(interception, [json!(true), json!(false)]);
    assert_eq!(
        calls
            .iter()
            .filter(|(m, p)| m == "Input.dispatchMouseEvent" && p["type"] == "mousePressed")
            .count(),
        1
    );
}

#[tokio::test]
async fn browser_pointer_drag_requires_release_without_synthetic_native_drop() {
    let (runtime, page, connection, _root) = fixture(Mode::Pointer).await;
    assert!(runtime
        .drag_action(&page, [0.0, 0.0, 100.0, 100.0], json!({}))
        .await
        .unwrap()
        .contains("pointer release event observed"));
    assert!(!connection
        .calls
        .lock()
        .iter()
        .any(|(m, _)| m == "Input.dispatchDragEvent"));
}

#[tokio::test(start_paused = true)]
async fn browser_uncertain_drag_is_not_replayed_and_disables_interception() {
    for mode in [Mode::NoDrop, Mode::NoIntercept] {
        let (runtime, page, connection, _root) = fixture(mode).await;
        assert!(runtime
            .drag_action(&page, [0.0, 0.0, 100.0, 100.0], json!({}))
            .await
            .unwrap_err()
            .starts_with("action_unconfirmed:"));
        let calls = connection.calls.lock();
        assert_eq!(
            calls
                .iter()
                .filter(|(m, p)| m == "Input.dispatchMouseEvent" && p["type"] == "mousePressed")
                .count(),
            1
        );
        assert!(calls.iter().any(|(m, _)| m == "Input.cancelDragging"));
        assert!(calls
            .iter()
            .any(|(m, p)| m == "Input.setInterceptDrags" && p["enabled"] == false));
        assert_eq!(
            calls
                .iter()
                .filter(|(m, p)| m == "Input.dispatchDragEvent" && p["type"] == "drop")
                .count(),
            usize::from(matches!(mode, Mode::NoDrop))
        );
    }
}

#[test]
fn browser_drag_rejects_file_and_unbounded_intercepted_data() {
    assert!(drag_data(&payload()).is_ok());
    assert!(drag_data(&json!({"items":[],"dragOperationsMask":-1})).is_ok());
    assert!(drag_data(&json!({"items":[]})).is_err());
    assert!(
        drag_data(&json!({"items":[],"files":["C:/private/file"],"dragOperationsMask":1})).is_err()
    );
    assert!(
        drag_data(&json!({"items":[],"dragOperationsMask":1,"extra":"x".repeat(1_048_577)}))
            .is_err()
    );
}
