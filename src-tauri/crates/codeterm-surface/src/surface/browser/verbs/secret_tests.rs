use super::*;
use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::EventFilter;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};

struct FieldConnection {
    maskable: bool,
    commands: Mutex<Vec<(String, Value)>>,
    field: Mutex<String>,
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
        EventStream::from_receiver(tokio::sync::mpsc::channel(1).1)
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
            "DOM.resolveNode" => json!({"object":{"objectId":"field"}}),
            "Runtime.callFunctionOn" => json!({"result":{"value":self.maskable}}),
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
            commands: Mutex::new(Vec::new()),
            field: Mutex::new(String::new()),
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
