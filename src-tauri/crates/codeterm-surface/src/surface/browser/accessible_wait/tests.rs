use super::*;
#[test]
fn accessible_wait_arguments_accept_aria_names_and_bounded_deadlines() {
    let args = [
        "--role",
        "button",
        "--name",
        "Aria-only label",
        "--state",
        "enabled",
        "--timeout",
        "500ms",
    ]
    .map(String::from);
    assert_eq!(
        parse(&args).unwrap(),
        AccessibleWait {
            role: "button".into(),
            name: "Aria-only label".into(),
            state: State::Enabled,
            timeout: Duration::from_millis(500)
        }
    );
    assert_eq!(duration("2m").unwrap(), Duration::from_secs(120));
    for value in ["0ms", "3m", "15", "NaNs", "18446744073709551615m"] {
        assert!(duration(value).is_err());
    }
    assert!(parse(&["--role".into(), "button".into()]).is_err());
}

use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::{CdpEvent, EventFilter};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct AccessibleConnection {
    captures: AtomicUsize,
    events: Mutex<Vec<tokio::sync::mpsc::Sender<Arc<CdpEvent>>>>,
    closed: tokio::sync::watch::Sender<Option<String>>,
}

#[async_trait::async_trait]
impl CdpConnection for AccessibleConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/injected"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, _: &str) {}
    fn subscribe(&self, _: EventFilter) -> EventStream {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        self.events.lock().push(tx);
        EventStream::from_receiver(rx)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        _: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"s"}),
            "Accessibility.getFullAXTree" => {
                let first = self.captures.fetch_add(1, Ordering::SeqCst) == 0;
                if first {
                    let event = Arc::new(CdpEvent {
                        method: "Runtime.bindingCalled".into(),
                        session_id: Some("s".into()),
                        params: json!({"name":BINDING,"payload":"changed"}),
                    });
                    for tx in self.events.lock().iter() {
                        tx.try_send(event.clone()).unwrap();
                    }
                }
                json!({"nodes":[{"nodeId":"aria-control","role":{"value":"button"},"name":{"value":"Aria-only label"},"backendDOMNodeId":7,"properties":[{"name":"disabled","value":{"value":first}}]}]})
            }
            "Runtime.evaluate" => json!({"result":{"value":null}}),
            "DOM.getContentQuads" => json!({"quads":[[0,0,20,0,20,20,0,20]]}),
            _ => json!({}),
        })
    }
}

#[tokio::test]
async fn aria_only_accessible_name_wait_rechecks_on_binding_evidence_without_text_or_polling() {
    let connection = Arc::new(AccessibleConnection {
        captures: AtomicUsize::new(0),
        events: Mutex::new(Vec::new()),
        closed: tokio::sync::watch::channel(None).0,
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
    let result = runtime
        .wait_accessible(
            &page,
            AccessibleWait {
                role: "button".into(),
                name: "Aria-only label".into(),
                state: State::Enabled,
                timeout: Duration::from_secs(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(result, "accessible state confirmed");
    assert_eq!(connection.captures.load(Ordering::SeqCst), 2);
}
