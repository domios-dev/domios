use super::IngressError;
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
pub(crate) trait ChannelIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<S: AsyncRead + AsyncWrite + Unpin + Send> ChannelIo for S {}

const CHUNK: usize = 48 * 1024;
const MAX_BODY: usize = 2 * 1024 * 1024;

pub(super) struct EncryptedStream<S> {
    socket: WebSocketStream<S>,
    cipher: snow::TransportState,
    pub binding: [u8; 32],
}

async fn read<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
) -> Result<Vec<u8>, IngressError> {
    match socket.next().await {
        Some(Ok(Message::Binary(bytes))) => Ok(bytes.into()),
        _ => Err(IngressError::Unavailable),
    }
}

async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    bytes: Vec<u8>,
) -> Result<(), IngressError> {
    socket
        .send(Message::Binary(bytes.into()))
        .await
        .map_err(|_| IngressError::Unavailable)
}

impl<S: AsyncRead + AsyncWrite + Unpin> EncryptedStream<S> {
    pub async fn handshake(
        socket: WebSocketStream<S>,
        initiator: bool,
    ) -> Result<Self, IngressError> {
        Self::handshake_with_domain(socket, initiator, b"domios-cloud-ingress-enrolled-v1").await
    }

    pub(super) async fn handshake_with_domain(
        mut socket: WebSocketStream<S>,
        initiator: bool,
        domain: &'static [u8],
    ) -> Result<Self, IngressError> {
        let params = "Noise_NN_25519_ChaChaPoly_SHA256"
            .parse()
            .map_err(|_| IngressError::Unavailable)?;
        let builder = snow::Builder::new(params)
            .prologue(domain)
            .map_err(|_| IngressError::Unavailable)?;
        let mut handshake = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(|_| IngressError::Unavailable)?;
        for writing in [initiator, !initiator] {
            if writing {
                let mut bytes = vec![0; 64];
                let len = handshake
                    .write_message(&[], &mut bytes)
                    .map_err(|_| IngressError::Rejected)?;
                bytes.truncate(len);
                send(&mut socket, bytes).await?;
            } else {
                let message = read(&mut socket).await?;
                if message.len() > 64 || handshake.read_message(&message, &mut []) != Ok(0) {
                    return Err(IngressError::Rejected);
                }
            }
        }
        let binding = handshake
            .get_handshake_hash()
            .try_into()
            .map_err(|_| IngressError::Rejected)?;
        let cipher = handshake
            .into_transport_mode()
            .map_err(|_| IngressError::Rejected)?;
        Ok(Self {
            socket,
            cipher,
            binding,
        })
    }

    pub async fn write<T: Serialize>(&mut self, value: &T) -> Result<(), IngressError> {
        let bytes = serde_json::to_vec(value).map_err(|_| IngressError::InvalidFrame)?;
        if bytes.is_empty() || bytes.len() > MAX_BODY {
            return Err(IngressError::InvalidFrame);
        }
        for chunk in bytes.chunks(CHUNK) {
            let mut plain = Vec::with_capacity(chunk.len() + 4);
            plain.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            plain.extend_from_slice(chunk);
            let mut encrypted = vec![0; plain.len() + 16];
            self.cipher
                .write_message(&plain, &mut encrypted)
                .map_err(|_| IngressError::Rejected)?;
            send(&mut self.socket, encrypted).await?;
        }
        Ok(())
    }

    pub async fn read<T: DeserializeOwned>(&mut self) -> Result<T, IngressError> {
        let mut bytes = Vec::new();
        let mut total = None;
        loop {
            let encrypted = read(&mut self.socket).await?;
            if !(21..=CHUNK + 20).contains(&encrypted.len()) {
                return Err(IngressError::InvalidFrame);
            }
            let mut plain = vec![0; encrypted.len()];
            let len = self
                .cipher
                .read_message(&encrypted, &mut plain)
                .map_err(|_| IngressError::Rejected)?;
            let announced = u32::from_be_bytes(
                plain[..4]
                    .try_into()
                    .map_err(|_| IngressError::InvalidFrame)?,
            ) as usize;
            if announced == 0
                || announced > MAX_BODY
                || total.replace(announced).is_some_and(|n| n != announced)
                || len - 4 != CHUNK.min(announced - bytes.len())
            {
                return Err(IngressError::InvalidFrame);
            }
            bytes.extend_from_slice(&plain[4..len]);
            if bytes.len() == announced {
                return serde_json::from_slice(&bytes).map_err(|_| IngressError::InvalidFrame);
            }
        }
    }
}
