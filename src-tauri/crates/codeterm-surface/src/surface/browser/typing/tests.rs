use super::*;
use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::EventFilter;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};

#[test]
fn submit_flag_never_becomes_inserted_text() {
    for args in [
        vec!["--submit", "textbox", "Search", "query"],
        vec!["textbox", "Search", "query", "--submit"],
    ] {
        let (words, submit) = parse(
            &args.into_iter().map(String::from).collect::<Vec<_>>(),
            false,
        )
        .unwrap();
        assert!(submit);
        assert_eq!(words, ["textbox", "Search", "query"]);
    }
    assert!(parse(&["--submit".into(), "@e1".into()], false).is_err());
    let (words, submit) =
        parse(&["textbox", "Password", "--submit"].map(String::from), true).unwrap();
    assert!(!submit);
    assert_eq!(words.last().unwrap(), "--submit");
    let (words, submit) = parse(&["@e1", "--submit", "secret"].map(String::from), true).unwrap();
    assert!(submit);
    assert_eq!(words, ["@e1", "secret"]);
}

#[test]
fn readback_verification_normalizes_whitespace_and_fails_closed_on_mismatch() {
    assert_eq!(
        normalize_rendered_text("\u{FEFF}Hello\u{00A0}world\n\n"),
        "Hello world"
    );
    assert_eq!(
        parse_readback(&json!({"result":{"value":{"ok":true,"text":"hi"}}}), false),
        Readback::Text("hi".into())
    );
    assert_eq!(
        parse_readback(&json!({"result":{"value":{"ok":true,"length":4}}}), true),
        Readback::SecretLength(4)
    );
    assert_eq!(
        parse_readback(
            &json!({"result":{"value":{"ok":false,"reason":"detached"}}}),
            false
        ),
        Readback::Invalid
    );

    assert!(verify_readback(
        "Hello world",
        false,
        &Readback::Text("\u{FEFF}Hello\u{00A0}world\n".into())
    )
    .is_ok());
    assert!(verify_readback(
        "Line 1\nLine 2",
        false,
        &Readback::Text("Line 1\n\nLine 2\n".into())
    )
    .is_ok());
    assert!(verify_readback("", false, &Readback::Text("\u{FEFF}\n".into())).is_ok());
    assert!(verify_readback("   ", false, &Readback::Text("\u{00A0} ".into())).is_ok());

    for (expected, observed) in [
        ("Hello world", ""),
        ("Hello world", "existing Hello world"),
        ("Hello world", "Hello"),
        ("", "still here"),
        ("   ", ""),
        ("   ", "\n"),
    ] {
        let err = verify_readback(expected, false, &Readback::Text(observed.into())).unwrap_err();
        assert!(err.starts_with("typing_unconfirmed:"), "{err}");
        assert!(err.contains("never replay"), "{err}");
    }

    let secret = "opaque-λ-🚀";
    assert_eq!(secret.encode_utf16().count(), 11);
    assert!(verify_readback(secret, true, &Readback::SecretLength(11)).is_ok());
    let err = verify_readback(secret, true, &Readback::SecretLength(5)).unwrap_err();
    assert!(err.starts_with("typing_unconfirmed:"), "{err}");
    assert!(!err.contains(secret));

    let err = verify_readback("hi", false, &Readback::Invalid).unwrap_err();
    assert!(err.starts_with("typing_unconfirmed:"), "{err}");
}

struct DraftEditorConnection {
    model: Mutex<String>,
    rendered: Mutex<String>,
    selected: Mutex<bool>,
    drop_render: bool,
    caret_lost_after_clear: bool,
    secret_truncated: bool,
    commands: Mutex<Vec<(String, Value)>>,
}

#[async_trait::async_trait]
impl CdpConnection for DraftEditorConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/draft"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        tokio::sync::watch::channel(None).1
    }
    fn close(&self, _: &str) {}
    fn subscribe(&self, _: EventFilter) -> EventStream {
        let (_tx, rx) = tokio::sync::mpsc::channel(4);
        EventStream::from_receiver(rx)
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
            "DOM.resolveNode" => json!({"object":{"objectId":"editor"}}),
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|src| src.contains("function domiosTypeFocus")) =>
            {
                let phase = params["arguments"]
                    .get(1)
                    .and_then(|arg| arg["value"].as_str());
                let secret = params["arguments"][0]["value"].as_bool().unwrap_or(false);
                if phase == Some("caret") {
                    let result = if self.caret_lost_after_clear {
                        "focus_lost"
                    } else {
                        "focused"
                    };
                    json!({"result":{"value":result}})
                } else if secret {
                    self.model.lock().clear();
                    self.rendered.lock().clear();
                    json!({"result":{"value":"focused"}})
                } else if !self.rendered.lock().is_empty() {
                    *self.selected.lock() = true;
                    json!({"result":{"value":"selected"}})
                } else {
                    json!({"result":{"value":"focused"}})
                }
            }
            "Input.dispatchKeyEvent"
                if params["type"] == "rawKeyDown" && params["key"] == "Backspace" =>
            {
                if *self.selected.lock() {
                    self.model.lock().clear();
                    self.rendered.lock().clear();
                    *self.selected.lock() = false;
                }
                json!({})
            }
            "Input.insertText" => {
                let text = params["text"].as_str().unwrap_or("");
                self.model.lock().push_str(text);
                if !self.drop_render {
                    if self.secret_truncated {
                        self.rendered.lock().push_str(&text[..text.len().min(3)]);
                    } else {
                        self.rendered.lock().push_str(text);
                    }
                }
                json!({})
            }
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|src| src.contains("function domiosTypeReadback")) =>
            {
                let secret = params["arguments"][0]["value"].as_bool().unwrap_or(false);
                let rendered = self.rendered.lock().clone();
                if secret {
                    json!({"result":{"value":{"ok":true,"length":rendered.encode_utf16().count()}}})
                } else {
                    json!({"result":{"value":{"ok":true,"text":rendered}}})
                }
            }
            _ => json!({}),
        })
    }
}

fn test_runtime() -> (tempfile::TempDir, BrowserRuntime) {
    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    (root, runtime)
}

#[tokio::test]
async fn draft_editor_clears_via_native_editing_and_fails_closed_on_rendered_mismatch() {
    let focus_js = include_str!("../type-focus.js");
    assert!(!focus_js.contains("textContent ="));
    assert!(!focus_js.contains("innerHTML ="));
    assert!(focus_js.contains("createRange"));
    assert!(focus_js.contains("getSelection"));

    let readback_js = include_str!("../type-readback.js");
    assert!(readback_js.contains("length:this.value.length"));

    let (_root, runtime) = test_runtime();

    let working = Arc::new(DraftEditorConnection {
        model: Mutex::new("existing draft".into()),
        rendered: Mutex::new("existing draft".into()),
        selected: Mutex::new(false),
        drop_render: false,
        caret_lost_after_clear: false,
        secret_truncated: false,
        commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(working.clone())
        .attach_page("target")
        .await
        .unwrap();
    runtime
        .type_into(&page, 7, "new post text", false)
        .await
        .unwrap();
    assert_eq!(working.model.lock().as_str(), "new post text");
    assert_eq!(working.rendered.lock().as_str(), "new post text");
    let cmds = working.commands.lock();
    let backspace = cmds
        .iter()
        .position(|(m, p)| {
            m == "Input.dispatchKeyEvent" && p["type"] == "rawKeyDown" && p["key"] == "Backspace"
        })
        .unwrap();
    let insert = cmds
        .iter()
        .position(|(m, _)| m == "Input.insertText")
        .unwrap();
    let readback = cmds
        .iter()
        .position(|(m, p)| {
            m == "Runtime.callFunctionOn"
                && p["functionDeclaration"]
                    .as_str()
                    .is_some_and(|src| src.contains("function domiosTypeReadback"))
        })
        .unwrap();
    let present = cmds
        .iter()
        .position(|(_, p)| p["awaitPromise"] == true)
        .unwrap();
    assert!(backspace < insert && insert < readback && readback < present);

    let mismatch = Arc::new(DraftEditorConnection {
        model: Mutex::new(String::new()),
        rendered: Mutex::new(String::new()),
        selected: Mutex::new(false),
        drop_render: true,
        caret_lost_after_clear: false,
        secret_truncated: false,
        commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(mismatch.clone())
        .attach_page("target")
        .await
        .unwrap();
    let err = runtime
        .type_into(&page, 7, "invisible draft", false)
        .await
        .unwrap_err();
    assert!(err.starts_with("typing_unconfirmed:"), "{err}");
    assert!(err.contains("never replay"), "{err}");
    let cmds = mismatch.commands.lock();
    assert_eq!(
        cmds.iter().filter(|(m, _)| m == "Input.insertText").count(),
        1
    );
    assert!(!cmds.iter().any(|(_, p)| p["awaitPromise"] == true));

    let secret = "opaque-secret-value";
    let truncated_secret = Arc::new(DraftEditorConnection {
        model: Mutex::new(String::new()),
        rendered: Mutex::new(String::new()),
        selected: Mutex::new(false),
        drop_render: false,
        caret_lost_after_clear: false,
        secret_truncated: true,
        commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(truncated_secret.clone())
        .attach_page("target")
        .await
        .unwrap();
    let err = runtime.type_into(&page, 7, secret, true).await.unwrap_err();
    assert!(err.starts_with("typing_unconfirmed:"), "{err}");
    assert!(!err.contains(secret));
    assert_eq!(
        truncated_secret
            .commands
            .lock()
            .iter()
            .filter(|(m, _)| m == "Input.insertText")
            .count(),
        1
    );

    let lost_caret = Arc::new(DraftEditorConnection {
        model: Mutex::new("old".into()),
        rendered: Mutex::new("old".into()),
        selected: Mutex::new(false),
        drop_render: false,
        caret_lost_after_clear: true,
        secret_truncated: false,
        commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(lost_caret.clone())
        .attach_page("target")
        .await
        .unwrap();
    let err = runtime
        .type_into(&page, 7, "after clear", false)
        .await
        .unwrap_err();
    assert!(err.starts_with("action_unconfirmed:"), "{err}");
    assert!(!lost_caret
        .commands
        .lock()
        .iter()
        .any(|(m, _)| m == "Input.insertText"));
}
