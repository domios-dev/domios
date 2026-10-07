use super::protocol::{decode, Frame, RequestId};
use crate::service::ServiceContext;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

#[derive(Default)]
struct Evidence {
    initialized: bool,
    pending: HashMap<RequestId, String>,
}

impl Evidence {
    fn request(&mut self, text: &str, thread: &str) {
        if let Ok(Frame::ServerRequest { id, method, params }) = decode(text) {
            if self.pending.len() < 64
                && (method == "initialize"
                    || (method == "thread/resume" && params["threadId"] == thread))
            {
                self.pending.insert(id, method);
            }
        }
    }

    fn response(&mut self, text: &str, thread: &str) -> bool {
        let Ok(Frame::Response { id, result }) = decode(text) else {
            return false;
        };
        let Some(method) = self.pending.remove(&id) else {
            return false;
        };
        let Ok(result) = result else { return false };
        if method == "initialize" {
            self.initialized = true;
            return false;
        }
        self.initialized && result["thread"]["id"] == thread
    }
}

pub fn start(
    svc: ServiceContext,
    tab: String,
    nonce: String,
    thread: String,
    token: String,
    upstream: u16,
    listener: tokio::net::TcpListener,
    mut shutdown: tokio::sync::watch::Receiver<Option<String>>,
) {
    tauri::async_runtime::spawn(async move {
        let connections = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
        loop {
            let accepted = tokio::select! {
                _ = shutdown.changed() => break,
                accepted = listener.accept() => accepted,
            };
            let Ok((stream, _)) = accepted else { break };
            let Ok(permit) = connections.clone().try_acquire_owned() else {
                continue;
            };
            let (svc, tab, nonce, thread, token, mut shutdown) = (
                svc.clone(),
                tab.clone(),
                nonce.clone(),
                thread.clone(),
                token.clone(),
                shutdown.clone(),
            );
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let connection = uuid::Uuid::new_v4().to_string();
                let relay = async {
                    let expected = format!("Bearer {token}");
                    let config =
                        tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                            .max_message_size(Some(8 * 1024 * 1024))
                            .max_frame_size(Some(8 * 1024 * 1024));
                    let socket = tokio::time::timeout(std::time::Duration::from_secs(15), tokio_tungstenite::accept_hdr_async_with_config(
                        stream,
                        move |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                            if request.headers().get("Authorization").and_then(|v| v.to_str().ok()) == Some(expected.as_str()) {
                                Ok(response)
                            } else {
                                Err(tokio_tungstenite::tungstenite::http::Response::builder()
                                    .status(401).body(Some("Unauthorized".into())).unwrap())
                            }
                        }, Some(config),
                    )).await.map_err(|_| "remote handshake deadline")?.map_err(|_| "remote authentication/handshake failed")?;
                    let mut request = format!("ws://127.0.0.1:{upstream}")
                        .into_client_request()
                        .map_err(|_| "invalid upstream address")?;
                    request.headers_mut().insert(
                        "Authorization",
                        format!("Bearer {token}")
                            .parse()
                            .map_err(|_| "invalid authorization")?,
                    );
                    let (server, _) = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        tokio_tungstenite::connect_async(request),
                    )
                    .await
                    .map_err(|_| "remote upstream deadline")?
                    .map_err(|_| "remote upstream connect failed")?;
                    let (mut remote_write, mut remote_read) = socket.split();
                    let (mut server_write, mut server_read) = server.split();
                    let mut evidence = Evidence::default();
                    loop {
                        tokio::select! {
                            message = remote_read.next() => {
                                let Some(Ok(message)) = message else { break };
                                if let Message::Text(text) = &message { evidence.request(text.as_ref(), &thread); }
                                server_write.send(message).await.map_err(|_| "remote upstream write failed")?;
                            },
                            message = server_read.next() => {
                                let Some(Ok(message)) = message else { break };
                                let attached = matches!(&message, Message::Text(text) if evidence.response(text.as_ref(), &thread));
                                remote_write.send(message).await.map_err(|_| "remote response write failed")?;
                                if attached {
                                    super::delivery::attached(&svc, &tab, &nonce, &connection);
                                }
                            }
                        }
                    }
                    Ok::<(), &str>(())
                };
                let result = tokio::select! {
                    _ = shutdown.changed() => Ok(()),
                    result = relay => result,
                };
                super::delivery::detached(&tab, &nonce, &connection);
                if let Err(reason) = result {
                    log::warn!("[codex-rpc-attach] tab={tab} nonce={nonce} reason={reason}");
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_rpc_attach_requires_own_initialize_and_bound_resume_success() {
        let mut evidence = Evidence::default();
        evidence.request(&json!({"id":1,"method":"initialize"}).to_string(), "own");
        assert!(!evidence.response(&json!({"id":1,"result":{}}).to_string(), "own"));
        evidence.request(
            &json!({"id":2,"method":"thread/resume","params":{"threadId":"other"}}).to_string(),
            "own",
        );
        assert!(!evidence.response(
            &json!({"id":2,"result":{"thread":{"id":"other"}}}).to_string(),
            "own"
        ));
        evidence.request(
            &json!({"id":3,"method":"thread/resume","params":{"threadId":"own"}}).to_string(),
            "own",
        );
        assert!(evidence.response(
            &json!({"id":3,"result":{"thread":{"id":"own"}}}).to_string(),
            "own"
        ));
    }
}
