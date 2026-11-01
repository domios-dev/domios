use super::*;

async fn deliver(
    params: serde_json::Value,
    accepted: bool,
) -> (Result<(), String>, String, String) {
    let content = outbound_content(&Some(params)).unwrap();
    let captured = Arc::new(PLMutex::new(None));
    let capture = captured.clone();
    let app = Router::new().fallback(post(move |uri: axum::http::Uri, body: axum::body::Bytes| {
        let capture = capture.clone();
        async move {
            *capture.lock() = Some((uri.path().to_string(), String::from_utf8(body.to_vec()).unwrap()));
            Json(serde_json::json!({"ok": accepted, "result": {"message_id": 42}, "description": "upload rejected"}))
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let tab: Arc<dyn crate::ports::BridgeTabPort> = Arc::new(NotificationBridgeTab {
        threads: Arc::new(crate::bridge::threads::ThreadService::new(
            crate::bridge::threads::ThreadRegistry::in_memory(),
        )),
        tabs: NotificationTabs,
        placements: PLMutex::new(Vec::new()),
        answer: None,
    });
    let (completion, delivered) = std::sync::mpsc::sync_channel(1);
    let message = OutboundMessage {
        user_id: Some(12345),
        content,
        append: false,
        buttons: None,
        edit: None,
        receipt: None,
        completion: Some(completion),
        route: OutboundRoute::Chat,
    };
    threaded::send_outbound(
        &tab,
        &reqwest::Client::new(),
        &base,
        "FAKE",
        12345,
        &message,
        &mut HashMap::new(),
    )
    .await;
    let result = delivered
        .try_recv()
        .expect("upload must acknowledge its outcome");
    let (method, body) = captured.lock().take().unwrap();
    server.abort();
    (result, method, body)
}

#[tokio::test]
async fn file_with_default_text_type_uploads_document_and_caption() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"opaque document bytes").unwrap();
    let (result, method, body) = deliver(
        serde_json::json!({
            "type": "text", "file": file.path(), "filename": "report.md",
            "text": "body fallback", "caption": "document caption"
        }),
        true,
    )
    .await;
    assert_eq!(result, Ok(()));
    assert_eq!(method, "/botFAKE/sendDocument");
    assert!(body.contains("name=\"document\"; filename=\"report.md\""));
    assert!(body.contains("opaque document bytes"));
    assert!(body.contains("name=\"caption\"\r\n\r\ndocument caption"));
    assert!(!body.contains("body fallback"));
}

#[tokio::test]
async fn caption_only_file_send_propagates_telegram_rejection() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let (result, method, _) = deliver(
        serde_json::json!({
            "file": file.path(), "caption": "caption without text"
        }),
        false,
    )
    .await;
    assert_eq!(method, "/botFAKE/sendDocument");
    assert_eq!(result, Err("sendDocument failed: upload rejected".into()));
}

#[test]
fn missing_file_is_an_error_even_when_text_is_present() {
    let directory = tempfile::tempdir().unwrap();
    let result = outbound_content(&Some(serde_json::json!({
        "type": "text", "file": directory.path().join("missing.md"), "text": "body"
    })));
    assert!(result.unwrap_err().starts_with("File not found:"));
}
