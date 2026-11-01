use super::*;

fn assert_callback_response(accepted: bool, return_to_app: bool) -> String {
    let response = page::response(accepted, return_to_app);
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert_eq!(
        headers.lines().next(),
        Some(if accepted {
            "HTTP/1.1 200 OK"
        } else {
            "HTTP/1.1 400 Bad Request"
        })
    );
    for header in [
        "Content-Type: text/html; charset=utf-8",
        "Cache-Control: no-store",
        "Connection: close",
        "Referrer-Policy: no-referrer",
        "X-Content-Type-Options: nosniff",
    ] {
        assert!(headers.lines().any(|line| line == header));
    }
    let policy = headers
        .lines()
        .find(|line| line.starts_with("Content-Security-Policy:"))
        .unwrap();
    assert!(policy.contains("default-src 'none'"));
    assert!(policy.contains("base-uri 'none'; form-action 'none'; frame-ancestors 'none'"));
    for (tag, directive) in [("style", "style-src"), ("script", "script-src")] {
        use base64::Engine;
        use sha2::Digest;
        let content = body
            .split_once(&format!("<{tag}>"))
            .unwrap()
            .1
            .split_once(&format!("</{tag}>"))
            .unwrap()
            .0;
        let hash = base64::engine::general_purpose::STANDARD
            .encode(sha2::Sha256::digest(content.as_bytes()));
        assert!(policy.contains(&format!("{directive} 'sha256-{hash}'")));
    }
    assert!(headers
        .lines()
        .any(|line| line == format!("Content-Length: {}", body.len())));
    assert!(body.contains("history.replaceState(null,'','/callback')"));
    assert!(body.len() < 4096);
    body.to_owned()
}

#[test]
fn callback_success_response_returns_to_production_without_query_secrets() {
    let code = "q".repeat(43);
    let state = "opaque-state-fixture-1234567890";
    let request = format!(
        "GET /callback?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1:43210\r\n\r\n"
    );
    let accepted = parse_request(request.as_bytes(), "http://127.0.0.1:43210/callback").is_ok();
    assert!(accepted);
    let body = assert_callback_response(accepted, true);
    assert!(body.contains("href=\"domios://focus\""));
    assert!(body.contains("window.location.replace(document.getElementById('open-app').href)"));
    assert!(!body.contains(&code));
    assert!(!body.contains(state));
}

#[test]
fn callback_success_response_for_other_instances_has_no_app_link() {
    let body = assert_callback_response(true, false);
    assert!(!body.contains("domios:"));
    assert!(!body.contains("window.location"));
}

#[test]
fn callback_refusal_response_never_opens_app_or_echoes_query() {
    let code = "refused-code-fixture";
    let state = "refused-state-fixture";
    let request = format!(
        "GET /callback?code={code}&state={state} HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n"
    );
    let accepted = parse_request(request.as_bytes(), "http://127.0.0.1:43210/callback").is_ok();
    assert!(!accepted);
    for return_to_app in [true, false] {
        let body = assert_callback_response(accepted, return_to_app);
        assert!(!body.contains("domios:"));
        assert!(!body.contains("window.location"));
        assert!(!body.contains(code));
        assert!(!body.contains(state));
    }
}

#[tokio::test]
async fn account_callback_listener_releases_its_port_while_children_live() {
    let callback = super::Loopback::bind().await.unwrap();
    let address = callback.listener.local_addr().unwrap();
    #[cfg(windows)]
    let child = tokio::task::spawn_blocking(
        codeterm_util::tcp_listener::test_support::InheritingChild::start,
    )
    .await
    .unwrap();
    drop(callback);
    let rebound = codeterm_util::tcp_listener::bind(address)
        .expect("the browser or an agent must not retain the login callback socket");
    #[cfg(windows)]
    assert!(child.is_alive());
    drop(rebound);
    #[cfg(windows)]
    tokio::task::spawn_blocking(move || drop(child))
        .await
        .unwrap();
}
#[test]
fn callback_request_requires_get_exact_path_and_single_parameters() {
    let request = format!(
        "GET /callback?code={}&state=fixture-state-value HTTP/1.1\r\nHost: 127.0.0.1:43210\r\n\r\n",
        "c".repeat(43)
    );
    let redirect = "http://127.0.0.1:43210/callback";
    assert!(parse_request(request.as_bytes(), redirect).is_ok());
    for invalid in [
        request.replace("GET ", "POST "),
        request.replace("Host: 127.0.0.1:43210\r\n", ""),
        request.replace("127.0.0.1:43210", "localhost:43210"),
        request.replace("127.0.0.1:43210", "127.0.0.1:43211"),
        request.replace("127.0.0.1:43210", "attacker.invalid"),
        request.replace("Host:", "Host: 127.0.0.1:43210\r\nHost:"),
        request.replace("/callback?", "/other?"),
        request.replace("&state=", "&code=duplicate&state="),
        request.replace("GET /", "GET https://external.invalid/"),
    ] {
        assert!(parse_request(invalid.as_bytes(), redirect).is_err());
    }
}
