use super::{
    CloudIngressAuthority, EnrollmentError, EnrollmentHandler, EnrollmentOperation, IngressError,
    IngressReady, MAX_ENROLLMENT_FRAME_BYTES,
};
use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        protocol::WebSocketConfig,
        Message,
    },
    WebSocketStream,
};

const MAX_SESSIONS: usize = 32;
const MAX_NOISE_MESSAGE: usize = u16::MAX as usize;

pub struct CloudIngressHandle(JoinHandle<()>);

impl Drop for CloudIngressHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn start(
    bind: SocketAddr,
    enrollment: Arc<dyn EnrollmentHandler>,
    authority: Arc<dyn CloudIngressAuthority>,
) -> (CloudIngressHandle, oneshot::Receiver<IngressReady>) {
    let (ready, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        if !bind.ip().is_loopback() && (!bind.is_ipv4() || bind.port() != 7687) {
            return;
        }
        let Ok(listener) = TcpListener::bind(bind).await else {
            return;
        };
        let Ok(bound_address) = listener.local_addr() else {
            return;
        };
        if ready.send(IngressReady { bound_address }).is_err() {
            return;
        }
        let mut sessions = JoinSet::new();
        loop {
            tokio::select! {
                _ = sessions.join_next(), if !sessions.is_empty() => {}
                accepted = listener.accept(), if sessions.len() < MAX_SESSIONS => {
                    let Ok((socket, _)) = accepted else { break };
                    let enrollment = enrollment.clone();
                    let authority = authority.clone();
                    sessions.spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(120), bootstrap(socket, enrollment, authority)).await;
                    });
                }
            }
        }
    });
    (CloudIngressHandle(task), receiver)
}

type Socket = WebSocketStream<TcpStream>;

fn upgrade(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    if request.uri().path_and_query().map(|p| p.as_str()) == Some("/cloud/enrollment/v1") {
        return Ok(response);
    }
    let mut refused = ErrorResponse::new(None);
    *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
    Err(refused)
}

async fn read(socket: &mut Socket) -> Result<Vec<u8>, IngressError> {
    match socket.next().await {
        Some(Ok(Message::Binary(bytes))) => Ok(bytes.to_vec()),
        _ => Err(IngressError::InvalidFrame),
    }
}

async fn send(socket: &mut Socket, data: Vec<u8>) -> Result<(), IngressError> {
    socket
        .send(Message::Binary(data.into()))
        .await
        .map_err(|_| IngressError::Unavailable)
}

async fn encrypted_reply(
    socket: &mut Socket,
    transport: &mut snow::TransportState,
    plaintext: &[u8],
) -> Result<(), IngressError> {
    if plaintext.len() > MAX_NOISE_MESSAGE - 16 {
        return Err(IngressError::InvalidFrame);
    }
    let mut ciphertext = vec![0; plaintext.len() + 16];
    transport
        .write_message(plaintext, &mut ciphertext)
        .map_err(|_| IngressError::InvalidFrame)?;
    send(socket, ciphertext).await
}

async fn bootstrap(
    socket: TcpStream,
    enrollment: Arc<dyn EnrollmentHandler>,
    authority: Arc<dyn CloudIngressAuthority>,
) -> Result<(), IngressError> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_ENROLLMENT_FRAME_BYTES))
        .max_frame_size(Some(MAX_ENROLLMENT_FRAME_BYTES));
    let mut socket = accept_hdr_async_with_config(socket, upgrade, Some(config))
        .await
        .map_err(|_| IngressError::Rejected)?;
    let offer = read(&mut socket).await?;
    let mut handshake = authority.bootstrap_responder()?.respond(&offer)?;
    send(&mut socket, handshake.reply.to_vec()).await?;
    let controller = read(&mut socket).await?;
    let session = handshake.authenticate_controller(authority.as_ref(), &controller)?;
    let responder = handshake.responder_proof(authority.as_ref(), &session)?;
    encrypted_reply(&mut socket, &mut handshake.transport, &responder).await?;
    loop {
        let ciphertext = read(&mut socket).await?;
        let mut message = vec![0; ciphertext.len()];
        let len = handshake
            .transport
            .read_message(&ciphertext, &mut message)
            .map_err(|_| IngressError::Rejected)?;
        let (tag, payload) = message[..len]
            .split_first()
            .ok_or(IngressError::InvalidFrame)?;
        let op = match tag {
            1 => EnrollmentOperation::Discover,
            2 => EnrollmentOperation::Apply,
            3 => EnrollmentOperation::Status,
            _ => return Err(IngressError::InvalidFrame),
        };
        authority.authorize_bootstrap_controller(
            session.remote_installation_key(),
            session.channel_binding(),
        )?;
        let response = match enrollment.handle(&session, op, payload).await {
            Ok(body) if body.len() < MAX_NOISE_MESSAGE - 16 => {
                let mut response = Vec::with_capacity(body.len() + 1);
                response.push(0);
                response.extend(body);
                response
            }
            Ok(_) | Err(EnrollmentError::InvalidFrame) => vec![1],
            Err(EnrollmentError::Rejected) => vec![2],
            Err(EnrollmentError::AlreadyApplied) => vec![3],
            Err(EnrollmentError::Unavailable) => vec![4],
        };
        encrypted_reply(&mut socket, &mut handshake.transport, &response).await?;
        if op == EnrollmentOperation::Apply {
            return Ok(());
        }
    }
}
