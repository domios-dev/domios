use super::*;

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
