use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn navigate_loaded(page: &PageChannel, url: &str) {
    let mut events = page.subscribe_session(&["Page.loadEventFired"]);
    page.send("Page.navigate", json!({"url": url}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), events.recv())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
#[ignore = "opens a real Chromium window on the local desktop; disposable profile only"]
async fn visible_signin_cookie_survives_headless_handoff() {
    let root = tempfile::tempdir().unwrap();
    let config = BrowserHostConfig::under_data_dir(root.path());
    config.layout.create_named("visible-proof").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (mut socket, _) = tokio::select! {
                accepted = listener.accept() => accepted.unwrap(),
                Some(_) = connections.join_next() => continue,
            };
            connections.spawn(async move {
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut chunk).await.unwrap();
                if count == 0 || request.len() > 8192 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8_lossy(&request);
            let setting = request.starts_with("GET /set ");
            let present = request.lines().any(|line| {
                line.to_ascii_lowercase().starts_with("cookie:")
                    && line.contains("visible_proof=retained")
            });
            let body = if present {
                "<title>Cookie retained</title>"
            } else {
                "<title>No cookie</title>"
            };
            let cookie = if setting {
                "Set-Cookie: visible_proof=retained; Max-Age=3600; Path=/; HttpOnly; SameSite=Lax\r\n"
            } else {
                ""
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n{cookie}Connection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let host = BrowserHost::new(config);
    let proof = async {
        host.open(OpenOptions {
            profile: "visible-proof".into(),
            width: 1000.0,
            height: 700.0,
            dpr: 1.0,
        })
        .await
        .unwrap();
        host.set_visible(true).await.unwrap();
        assert!(host.is_visible());
        let page = host.page().unwrap();
        navigate_loaded(&page, &format!("http://{address}/set")).await;
        navigate_loaded(&page, &format!("http://{address}/check")).await;
        let mut exited = host.exit_watch().unwrap();
        let _ = host
            .current()
            .unwrap()
            .client
            .send("Browser.close", json!({}))
            .await;
        tokio::time::timeout(Duration::from_secs(15), exited.wait_for(Option::is_some))
            .await
            .unwrap()
            .unwrap();
        host.set_visible(false).await.unwrap();
        assert!(!host.is_visible());
        let page = host.page().unwrap();
        navigate_loaded(&page, &format!("http://{address}/check")).await;
        let title = page
            .send(
                "Runtime.evaluate",
                json!({"expression":"document.title", "returnByValue":true}),
            )
            .await
            .unwrap();
        assert_eq!(
            title
                .pointer("/result/value")
                .and_then(serde_json::Value::as_str),
            Some("Cookie retained")
        );
    };
    let result = std::panic::AssertUnwindSafe(proof);
    use futures_util::FutureExt;
    let result = result.catch_unwind().await;
    let close = host.shutdown().await;
    server.abort();
    let _ = server.await;
    assert!(close.is_some_and(CloseOutcome::confirms_exit));
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
