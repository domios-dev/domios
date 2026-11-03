use super::*;
use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::{CdpEvent, EventFilter};
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Notify};

struct Connection {
    calls: Mutex<Vec<(String, Value)>>,
    subscribers: Mutex<Vec<mpsc::Sender<Arc<CdpEvent>>>>,
    closed: watch::Sender<Option<String>>,
    animation_started: Notify,
    animation_ended: Notify,
    fail_input: bool,
    stall_evaluation: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl CdpConnection for Connection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/presentation"
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
    }
    fn subscribe(&self, _: EventFilter) -> EventStream {
        let (tx, rx) = mpsc::channel(64);
        self.subscribers.lock().push(tx);
        EventStream::from_receiver(rx)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        self.calls.lock().push((method.into(), params.clone()));
        if method == "Runtime.evaluate"
            && self
                .stall_evaluation
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.animation_started.notify_one();
            return std::future::pending().await;
        }
        if params["awaitPromise"] == true {
            self.animation_started.notify_one();
            self.animation_ended.notified().await;
        }
        if self.fail_input && method == "Input.dispatchMouseEvent" {
            return Err(CdpError::Closed("uncertain input".into()));
        }
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}}),
            "Page.getLayoutMetrics" => {
                json!({"cssLayoutViewport":{"pageX":0,"pageY":222,"clientWidth":380,"clientHeight":844}})
            }
            "DOM.getContentQuads" => json!({"quads":[[10,20,30,20,30,40,10,40]]}),
            _ => json!({"result":{"value":true}}),
        })
    }
}

async fn fixture(
    fail_input: bool,
) -> (
    BrowserRuntime,
    PageChannel,
    Arc<Connection>,
    tempfile::TempDir,
) {
    let connection = Arc::new(Connection {
        calls: Mutex::new(Vec::new()),
        subscribers: Mutex::new(Vec::new()),
        closed: watch::channel(None).0,
        animation_started: Notify::new(),
        animation_ended: Notify::new(),
        fail_input,
        stall_evaluation: std::sync::atomic::AtomicBool::new(false),
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("target")
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(dir.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    runtime.shared.bind(&page);
    (runtime, page, connection, dir)
}

#[tokio::test]
async fn browser_viewer_input_dispatches_while_cursor_evaluation_never_answers() {
    let (runtime, page, wire, _dir) = fixture(false).await;
    wire.calls.lock().clear();
    wire.stall_evaluation
        .store(true, std::sync::atomic::Ordering::SeqCst);
    runtime
        .viewer_input(&page, &super::super::input::InputArgs::key("a", &[]))
        .await
        .unwrap();
    assert!(!wire
        .calls
        .lock()
        .iter()
        .any(|(method, _)| method == "Runtime.evaluate"));
    runtime.shared.show_cursor(&page);
    let wheel = super::super::input::InputArgs {
        kind: "wheel".into(),
        x: Some(20.0),
        y: Some(30.0),
        delta_y: Some(100.0),
        ..Default::default()
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        runtime.viewer_input(&page, &wheel).await.unwrap();
        runtime
            .viewer_input(&page, &super::super::input::InputArgs::key("b", &[]))
            .await
            .unwrap();
    })
    .await
    .expect("viewer input must not wait for page evaluation");
    tokio::time::timeout(Duration::from_secs(1), wire.animation_started.notified())
        .await
        .expect("cursor hide was dispatched");
    let calls = wire.calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .count(),
        1
    );
    assert!(calls
        .iter()
        .any(|(method, params)| method == "Input.dispatchMouseEvent"
            && params["type"] == "mouseWheel"));
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "Input.dispatchKeyEvent")
            .count(),
        4
    );
}

#[tokio::test]
async fn browser_overlay_waits_for_animation_and_never_replays_uncertain_input() {
    for fail_input in [false, true] {
        let (runtime, page, wire, _dir) = fixture(fail_input).await;
        let action = runtime.click_node(&page, 7);
        tokio::pin!(action);
        tokio::select! {
            result = &mut action => panic!("input ran before animation completion: {result:?}"),
            _ = wire.animation_started.notified() => {}
        }
        assert!(!wire
            .calls
            .lock()
            .iter()
            .any(|(method, _)| method.starts_with("Input.")));
        wire.animation_ended.notify_one();
        let result = action.await;
        assert_eq!(result.is_err(), fail_input);
        let calls = wire.calls.lock();
        let inputs: Vec<_> = calls
            .iter()
            .filter(|(method, _)| method == "Input.dispatchMouseEvent")
            .collect();
        assert_eq!(inputs.len(), if fail_input { 1 } else { 3 });
        for (_, params) in inputs {
            assert_eq!(
                (params["x"].as_f64(), params["y"].as_f64()),
                (Some(20.0), Some(30.0))
            );
        }
        assert_eq!(
            calls
                .iter()
                .filter(|(_, params)| params["awaitPromise"] == true)
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn browser_overlay_injection_is_per_session_and_evidence_capture_hides_it() {
    let (runtime, page, wire, _dir) = fixture(false).await;
    runtime.install_overlay(&page).await.unwrap();
    runtime.install_overlay(&page).await.unwrap();
    runtime
        .capture_screenshot(&page, json!({"format":"png"}), false)
        .await
        .unwrap();
    runtime
        .capture_screenshot(&page, json!({"format":"png"}), true)
        .await
        .unwrap();
    let calls = wire.calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "Page.addScriptToEvaluateOnNewDocument")
            .count(),
        1
    );
    let context_script = &calls
        .iter()
        .find(|(method, _)| method == "Page.addScriptToEvaluateOnNewDocument")
        .unwrap()
        .1["source"];
    assert!(context_script
        .as_str()
        .unwrap()
        .contains("Symbol.for('domios.browser.target')"));
    assert!(context_script
        .as_str()
        .unwrap()
        .contains(&serde_json::to_string(page.target_id()).unwrap()));
    assert!(calls
        .iter()
        .any(|(method, params)| method == "Runtime.evaluate"
            && params["expression"] == *context_script));
    let capture: Vec<_> = calls
        .iter()
        .filter(|(method, params)| {
            method == "Page.captureScreenshot"
                || params["expression"]
                    .as_str()
                    .is_some_and(|s| s.contains("?.capture("))
        })
        .collect();
    assert_eq!(capture.len(), 4);
    assert!(capture[0].1["expression"]
        .as_str()
        .unwrap()
        .ends_with("capture(true)"));
    assert_eq!(capture[1].0, "Page.captureScreenshot");
    assert_eq!(
        capture[1].1["clip"],
        json!({"x":0.0,"y":222.0,"width":380.0,"height":844.0,"scale":1})
    );
    assert!(capture[2].1["expression"]
        .as_str()
        .unwrap()
        .ends_with("capture(false)"));
    assert_eq!(capture[3].0, "Page.captureScreenshot");
    assert_eq!(capture[3].1["clip"], capture[1].1["clip"]);
}
