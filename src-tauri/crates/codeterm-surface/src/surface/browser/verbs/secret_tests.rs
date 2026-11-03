use super::*;
use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::EventFilter;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};

struct FieldConnection {
    maskable: bool,
    submitted: bool,
    commands: Mutex<Vec<(String, Value)>>,
    field: Mutex<String>,
    subscribers: Mutex<Vec<tokio::sync::mpsc::Sender<Arc<codeterm_browser::cdp_wire::CdpEvent>>>>,
}

#[async_trait::async_trait]
impl CdpConnection for FieldConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/injected"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        tokio::sync::watch::channel(None).1
    }
    fn close(&self, _: &str) {}
    fn subscribe(&self, _: EventFilter) -> EventStream {
        let (sender, receiver) = tokio::sync::mpsc::channel(16);
        self.subscribers.lock().push(sender);
        EventStream::from_receiver(receiver)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        self.commands.lock().push((method.into(), params.clone()));
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}}),
            "DOM.resolveNode" => json!({"object":{"objectId":"field"}}),
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|source| source.contains("function domiosVisible")) =>
            {
                json!({"result":{"value":true}})
            }
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|source| source.contains("function domiosSubmit")) =>
            {
                json!({"result":{"value":match params["arguments"][0]["value"].as_str() {Some("begin") => "ready", Some("finish") if self.submitted => "submitted", Some("finish") => "no_form", _ => "cleaned"}}})
            }
            "Runtime.callFunctionOn" => {
                json!({"result":{"value":if self.maskable {"focused"} else {"unmaskable"}}})
            }
            "Input.insertText" => {
                *self.field.lock() = params["text"].as_str().unwrap().into();
                json!({})
            }
            _ => json!({}),
        })
    }
}

#[tokio::test]
async fn stdin_typing_masks_before_insertion_and_refuses_unmaskable_fields() {
    for maskable in [true, false] {
        let connection = Arc::new(FieldConnection {
            maskable,
            submitted: false,
            commands: Mutex::new(Vec::new()),
            field: Mutex::new(String::new()),
            subscribers: Default::default(),
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
        let secret = "opaque-input-λ";
        let result = runtime.type_into(&page, 7, secret, true).await;
        assert_eq!(result.is_ok(), maskable);
        assert_eq!(
            connection.field.lock().as_str(),
            if maskable { secret } else { "" }
        );
        let commands = connection.commands.lock();
        let mask = commands
            .iter()
            .position(|(method, params)| {
                method == "Runtime.callFunctionOn"
                    && params["functionDeclaration"]
                        .as_str()
                        .unwrap()
                        .contains("this.type='password'")
            })
            .unwrap();
        let insert = commands
            .iter()
            .position(|(method, _)| method == "Input.insertText");
        if maskable {
            assert!(mask < insert.unwrap());
        } else {
            assert!(insert.is_none());
        }
    }
}

#[tokio::test]
async fn type_submit_requires_evidence_and_never_replays_insert_or_enter() {
    for submitted in [true, false] {
        let connection = Arc::new(FieldConnection {
            maskable: true,
            submitted,
            commands: Default::default(),
            field: Default::default(),
            subscribers: Default::default(),
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
        let capture = runtime.shared.begin_snapshot(page.session_id()).unwrap();
        let nodes = codeterm_browser::snapshot::parse_full_ax_tree(&json!({"nodes":[{"nodeId":"input","role":{"value":"textbox"},"name":{"value":"Search"},"backendDOMNodeId":7}]})).unwrap();
        runtime
            .shared
            .finish_snapshot(
                &capture,
                codeterm_browser::snapshot::build_snapshot_scoped(&nodes, 10, &capture),
            )
            .unwrap();
        let result = runtime
            .type_verb(
                &page,
                &["--submit", "textbox", "Search", "one insertion"].map(String::from),
                false,
            )
            .await;
        assert_eq!(result.is_ok(), submitted, "{result:?}");
        if !submitted {
            assert!(result.unwrap_err().starts_with("submit_unconfirmed:"));
        }
        let commands = connection.commands.lock();
        assert_eq!(
            commands
                .iter()
                .filter(|(method, _)| method == "Input.insertText")
                .count(),
            1
        );
        assert_eq!(
            commands
                .iter()
                .filter(|(method, params)| method == "Input.dispatchKeyEvent"
                    && params["type"] == "keyDown"
                    && params["key"] == "Enter")
                .count(),
            1
        );
    }
}
