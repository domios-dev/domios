//! WHAT: Repeated requests from independently created mesh clients reuse one TCP connection.
//! WHY: Per-request `reqwest::Client` construction discarded pools and caused connection churn.
//! ORACLE: The server observes the same remote socket port for all 12 requests in each client pool.

use super::*;
use axum::{extract::ConnectInfo, routing::get, Json, Router};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const REQUESTS: usize = 12;
const REQUEST_DEADLINE: Duration = Duration::from_secs(5);

async fn connection_port(ConnectInfo(address): ConnectInfo<SocketAddr>) -> Json<u16> {
    Json(address.port())
}

async fn start_connection_server() -> (SocketAddr, oneshot::Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let router = Router::new().route("/connection-port", get(connection_port));
    let (shutdown, stop) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stop.await;
        })
        .await
        .expect("serve test router");
    });
    (address, shutdown, server)
}

#[tokio::test]
async fn repeated_mesh_client_construction_reuses_one_keep_alive_connection() {
    let (address, shutdown, server) = start_connection_server().await;
    let mut peer_ports = Vec::with_capacity(REQUESTS);
    let mut local_ports = Vec::with_capacity(REQUESTS);

    for _ in 0..REQUESTS {
        let client = codeterm_service_core::mesh::transport::PeerClient::new_for_test_without_auth(
            "127.0.0.1",
            address.port(),
        );
        let body = tokio::time::timeout(REQUEST_DEADLINE, client.auth_get_json("/connection-port"))
            .await
            .expect("mesh request deadline")
            .expect("mesh request succeeds");
        peer_ports.push(body.as_u64().expect("peer socket port") as u16);

        let response = tokio::time::timeout(
            REQUEST_DEADLINE,
            local_forward_http_client()
                .get(format!("http://{address}/connection-port"))
                .timeout(REQUEST_DEADLINE)
                .send(),
        )
        .await
        .expect("local forward request deadline")
        .expect("local forward request succeeds");
        let port = tokio::time::timeout(REQUEST_DEADLINE, response.json::<u16>())
            .await
            .expect("local response body deadline")
            .expect("local response body is a socket port");
        local_ports.push(port);
    }

    assert!(
        peer_ports.windows(2).all(|pair| pair[0] == pair[1]),
        "mesh requests used multiple TCP connections: {peer_ports:?}"
    );
    assert!(
        local_ports.windows(2).all(|pair| pair[0] == pair[1]),
        "local relay requests used multiple TCP connections: {local_ports:?}"
    );

    let _ = shutdown.send(());
    server.await.expect("test server task");
}
