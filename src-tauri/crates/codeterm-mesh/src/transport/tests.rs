use super::*;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{io::AsyncWriteExt, net::TcpListener};

const CRON_PATH: &str = "/api/crons/disposable-b/trigger";

async fn launch_relay_endpoint(
    status: &str,
    answer: Option<Value>,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("endpoint bind");
    let url = format!("http://{}", listener.local_addr().expect("local endpoint"));
    let admissions = Arc::new(AtomicUsize::new(0));
    let seen = admissions.clone();
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("relay connect");
            let bytes = read_http_request(&mut socket).await;
            let wire = std::str::from_utf8(&bytes).expect("relay request is UTF-8");
            let (head, body) = wire.split_once("\r\n\r\n").expect("complete relay request");
            assert!(head.starts_with("POST /api/mesh/relay HTTP/1.1\r\n"));
            assert!(head
                .lines()
                .any(|line| line
                    .eq_ignore_ascii_case("authorization: Bearer signed-initiating-actor")));
            let payload: Value = serde_json::from_str(body).expect("relay command");
            assert_eq!(payload["method"], "POST");
            assert_eq!(payload["path"], CRON_PATH);
            seen.fetch_add(1, Ordering::SeqCst);
            let body = answer.as_ref().map(Value::to_string).unwrap_or_default();
            let length = if answer.is_some() { body.len() } else { 128 };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("reply headers");
            socket.shutdown().await.expect("close response stream");
        }
    });
    (url, admissions, task)
}

async fn trigger(
    client: &PeerClient,
) -> Result<super::super::types::RelayResponse, TransportError> {
    let command = super::super::types::RelayCommand {
        method: "POST".into(),
        path: CRON_PATH.into(),
        body: Value::Null,
        delivery_id: None,
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.send_relay(&command),
    )
    .await
    .expect("local relay must settle")
}

#[tokio::test]
async fn a_lost_cron_reply_does_not_admit_it_on_an_alternative_endpoint() {
    let (primary, primary_admissions, primary_task) = launch_relay_endpoint("200 OK", None).await;
    let (alternative, alternative_admissions, alternative_task) = launch_relay_endpoint(
        "200 OK",
        Some(json!({"ok":true,"status":200,"data":{"run_id":"duplicate"}})),
    )
    .await;
    let client = PeerClient::new_signed_url(primary, "signed-initiating-actor")
        .with_relay_alternatives(vec![alternative]);
    let result = trigger(&client)
        .await
        .expect_err("partial response is uncertain");
    assert_eq!(result.kind(), TransportErrorKind::SubmitUnconfirmed);
    assert_eq!(result.bytes_written(), None);
    assert_eq!(primary_admissions.load(Ordering::SeqCst), 1);
    assert_eq!(alternative_admissions.load(Ordering::SeqCst), 0);
    primary_task.abort();
    alternative_task.abort();
}

#[tokio::test]
async fn a_prewrite_connect_failure_allows_one_cron_admission_and_preserves_the_receipt() {
    let receipt = json!({"run_id":"committed-on-b"});
    let (alternative, admissions, task) = launch_relay_endpoint(
        "200 OK",
        Some(json!({"ok":true,"status":200,"data":receipt})),
    )
    .await;
    let client = PeerClient::new_signed("127.0.0.1", 1, "signed-initiating-actor")
        .with_relay_alternatives(vec![alternative]);
    let result = trigger(&client)
        .await
        .expect("alternative can receive the first request");
    assert!(result.ok);
    assert_eq!(result.status, 200);
    assert_eq!(result.data, Some(receipt));
    assert_eq!(admissions.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn a_typed_relay_denial_is_authoritative_even_with_an_alternative() {
    let denial = json!({"error": {"code":"peer_relay_not_authorized","message":"grant changed", "bytes_written":false}});
    let (primary, primary_requests, primary_task) =
        launch_relay_endpoint("403 Forbidden", Some(denial.clone())).await;
    let (alternative, alternative_requests, alternative_task) =
        launch_relay_endpoint("200 OK", Some(json!({"ok":true,"status":200}))).await;
    let client = PeerClient::new_signed_url(primary, "signed-initiating-actor")
        .with_relay_alternatives(vec![alternative]);
    let result = trigger(&client).await.expect("denial is an answer");
    assert!(!result.ok);
    assert_eq!(result.status, 403);
    assert_eq!(result.data, Some(denial));
    assert_eq!(primary_requests.load(Ordering::SeqCst), 1);
    assert_eq!(alternative_requests.load(Ordering::SeqCst), 0);
    primary_task.abort();
    alternative_task.abort();
}

async fn read_http_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;

    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = socket.read(&mut chunk).await.expect("read request");
        assert_ne!(count, 0, "client closed before sending its request");
        request.extend_from_slice(&chunk[..count]);
        let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .expect("request content length");
        if request.len() >= header_end + content_length {
            return request;
        }
    }
}

/// The client default is the floor for trivial control calls. A liveness
/// probe has to outlive it, or a peer that is merely loaded gets declared
/// dead — which is what painted a 1ms-away machine offline (#319).
#[test]
fn a_probe_outlives_the_default_control_budget() {
    assert!(
        PING_TIMEOUT > CLIENT_DEFAULT_TIMEOUT,
        "a probe that expires with an ordinary control call cannot tell a slow peer from a dead one"
    );
}

/// A relayed command carries the peer's actual work, so its budget is about
/// patience with a stall — never about how long the work may take.
#[test]
fn a_relayed_command_outlives_a_probe() {
    assert!(
        RELAY_TIMEOUT > PING_TIMEOUT,
        "relayed work bounded by the probe budget turns a slow peer into a 502"
    );
}

/// A connection refused before any request bytes are written must classify
/// as nothing-sent, never as an undecidable outcome — the caller can safely
/// retry. Drives a real `reqwest::Error` rather than fabricating one, so the
/// `is_connect()`/`is_timeout()` classification itself is under test.
#[tokio::test]
async fn connection_refused_before_write_is_nothing_sent() {
    // Probe loopback port 1 because refusal is synchronous and avoids a bind/drop race.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client");
    let error = client
        .post("http://127.0.0.1:1/api/mesh/relay")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect_err("nothing listens on 127.0.0.1:1");
    assert!(
        error.is_connect(),
        "test setup bug: expected a connect-phase error, got {error:?}"
    );

    let transport_error = TransportError::from_send_error(&error);
    assert_eq!(
        transport_error.code(),
        crate::contract::delivery::MESH_TRANSPORT_NOTHING_SENT
    );
    assert_eq!(
        transport_error.bytes_written(),
        Some(false),
        "bytes_written must survive as a definite false, not silently drop"
    );
    assert_ne!(
        transport_error.code(),
        crate::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
        "nothing-landed must never be confused with landed-unknown"
    );
}

/// A timeout after the request was written must classify as landed-unknown
/// (`submit_unconfirmed`), never as nothing-sent.
#[tokio::test]
async fn timeout_after_write_is_submit_unconfirmed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let (request_tx, request_rx) = tokio::sync::oneshot::channel();

    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("relay connection");
        let request = read_http_request(&mut socket).await;
        request_tx.send(request).expect("test awaits the request");
        std::future::pending::<()>().await;
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(300))
        .build()
        .expect("client");
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(format!("http://{addr}/api/mesh/relay"))
            .json(&serde_json::json!({}))
            .send(),
    )
    .await
    .expect("client timeout must finish within the test deadline")
    .expect_err("the peer never answers, so the client timeout must fire");
    let request = tokio::time::timeout(std::time::Duration::from_secs(2), request_rx)
        .await
        .expect("peer must receive the request within the test deadline")
        .expect("peer task");
    assert!(request.starts_with(b"POST /api/mesh/relay HTTP/1.1\r\n"));
    peer.abort();
    assert!(
        error.is_timeout(),
        "test setup bug: expected a timeout error, got {error:?}"
    );

    let transport_error = TransportError::from_send_error(&error);
    assert_eq!(
        transport_error.code(),
        crate::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
    );
    assert_eq!(
        transport_error.bytes_written(),
        None,
        "the payload may already be in flight — never claim zero bytes here"
    );
    assert_ne!(
        transport_error.code(),
        crate::contract::delivery::MESH_TRANSPORT_NOTHING_SENT,
        "landed-unknown must never be confused with nothing-landed"
    );
}
