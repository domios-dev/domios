use super::*;
fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}
#[test]
fn atomic_locator_arguments_require_complete_scope_and_refuse_duplicates() {
    let out = parse(
        &args(&[
            "--within",
            "dialog:Settings",
            "--role",
            "textbox",
            "--name",
            "Title",
            "new value",
        ]),
        true,
    )
    .unwrap();
    assert_eq!(out.text.as_deref(), Some("new value"));
    assert_eq!(out.within, "dialog:Settings");
    for words in [
        vec!["--role", "button"],
        vec!["--within"],
        vec![
            "--within", "dialog:A", "--within", "dialog:B", "--role", "button", "--name", "Save",
        ],
    ] {
        assert!(parse(&args(&words), false).is_err());
    }
}

use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::EventFilter;
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct DisclosureConnection {
    expanded: AtomicBool,
    actions: AtomicUsize,
    uncertain: bool,
    closed: tokio::sync::watch::Sender<Option<String>>,
}
#[async_trait::async_trait]
impl CdpConnection for DisclosureConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/injected"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, _: &str) {}
    fn subscribe(&self, _: EventFilter) -> EventStream {
        EventStream::from_receiver(tokio::sync::mpsc::channel(1).1)
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
            "Runtime.evaluate" => json!({"result":{"value":0}}),
            "Accessibility.getFullAXTree" => {
                json!({"nodes":[{"nodeId":"d","role":{"value":"DisclosureTriangle"},"name":{"value":"Details"},"backendDOMNodeId":7,"properties":[{"name":"expanded","value":{"value":self.expanded.load(Ordering::SeqCst)}}]}]})
            }
            "DOM.resolveNode" => json!({"object":{"objectId":"summary"}}),
            "Runtime.callFunctionOn" => {
                self.actions.fetch_add(1, Ordering::SeqCst);
                self.expanded.fetch_xor(true, Ordering::SeqCst);
                if self.uncertain {
                    return Err(CdpError::Deadline(method.into()));
                }
                json!({"result":{"value":"performed"}})
            }
            _ => json!({}),
        })
    }
}
#[tokio::test]
async fn disclosure_actions_read_back_state_and_never_replay_uncertain_clicks() {
    for uncertain in [false, true] {
        let connection = Arc::new(DisclosureConnection {
            expanded: AtomicBool::new(false),
            actions: AtomicUsize::new(0),
            uncertain,
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
        runtime.shared.bind(&page);
        let args = ["summary", "Details"].map(String::from);
        let result = runtime.disclosure_verb(&page, &args, true).await;
        assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
        assert!(connection.expanded.load(Ordering::SeqCst));
        if uncertain {
            assert!(result.unwrap_err().starts_with("action_unconfirmed:"));
        } else {
            assert_eq!(result.unwrap(), "expanded state confirmed");
            assert_eq!(
                runtime.disclosure_verb(&page, &args, true).await.unwrap(),
                "disclosure state confirmed; no action needed"
            );
            assert_eq!(connection.actions.load(Ordering::SeqCst), 1);
            assert_eq!(
                runtime.disclosure_verb(&page, &args, false).await.unwrap(),
                "collapsed state confirmed"
            );
            assert_eq!(connection.actions.load(Ordering::SeqCst), 2);
            assert!(!connection.expanded.load(Ordering::SeqCst));
        }
    }
}
