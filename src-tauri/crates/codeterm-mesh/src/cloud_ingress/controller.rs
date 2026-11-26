use super::dial::{open, ClientSocket};
use super::{
    ChannelProof, CloudRoute, EnrollmentError, EnrollmentOperation, EnrollmentSession, IngressError,
};
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

pub struct EnrollmentClient {
    pub session: EnrollmentSession,
    socket: Option<ClientSocket>,
    transport: snow::TransportState,
}

async fn read(socket: &mut ClientSocket) -> Result<Vec<u8>, IngressError> {
    match socket.next().await {
        Some(Ok(Message::Binary(data))) => Ok(data.to_vec()),
        _ => Err(IngressError::Unavailable),
    }
}

pub async fn connect_enrollment(
    route: CloudRoute<'_>,
    creation_psk: &[u8; 32],
    controller_key: [u8; 32],
    sign: impl FnOnce(&ChannelProof) -> Result<[u8; 64], IngressError>,
) -> Result<EnrollmentClient, IngressError> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut socket = open(route, "/cloud/enrollment/v1").await?;
        let params = "Noise_NNpsk0_25519_ChaChaPoly_SHA256"
            .parse()
            .map_err(|_| IngressError::Unavailable)?;
        let mut handshake = snow::Builder::new(params)
            .prologue(b"domios-cloud-ingress-bootstrap-v1")
            .and_then(|b| b.psk(0, creation_psk))
            .and_then(snow::Builder::build_initiator)
            .map_err(|_| IngressError::Unavailable)?;
        let mut offer = vec![0; 48];
        let len = handshake
            .write_message(&[], &mut offer)
            .map_err(|_| IngressError::Unavailable)?;
        offer.truncate(len);
        socket
            .send(Message::Binary(offer.into()))
            .await
            .map_err(|_| IngressError::Unavailable)?;
        let reply = read(&mut socket).await?;
        if reply.len() != 48 || handshake.read_message(&reply, &mut []).is_err() {
            return Err(IngressError::Rejected);
        }
        let binding = handshake
            .get_handshake_hash()
            .try_into()
            .map_err(|_| IngressError::Rejected)?;
        let mut transport = handshake
            .into_transport_mode()
            .map_err(|_| IngressError::Unavailable)?;
        let proof = ChannelProof::bootstrap(
            b"domios-cloud-ingress-bootstrap-controller-v1\0",
            &binding,
            &controller_key,
        );
        let mut message = controller_key.to_vec();
        message.extend_from_slice(&sign(&proof)?);
        let mut ciphertext = vec![0; message.len() + 16];
        transport
            .write_message(&message, &mut ciphertext)
            .map_err(|_| IngressError::Unavailable)?;
        socket
            .send(Message::Binary(ciphertext.into()))
            .await
            .map_err(|_| IngressError::Unavailable)?;
        let response = read(&mut socket).await?;
        let mut frame = [0; 96];
        if response.len() != 112 || transport.read_message(&response, &mut frame) != Ok(96) {
            return Err(IngressError::Rejected);
        }
        let remote_key = frame[..32]
            .try_into()
            .map_err(|_| IngressError::InvalidFrame)?;
        let signature =
            Signature::from_slice(&frame[32..]).map_err(|_| IngressError::InvalidFrame)?;
        let proof = ChannelProof::bootstrap(
            b"domios-cloud-ingress-bootstrap-responder-v1\0",
            &binding,
            &remote_key,
        );
        VerifyingKey::from_bytes(&remote_key)
            .map_err(|_| IngressError::Rejected)?
            .verify_strict(proof.as_bytes(), &signature)
            .map_err(|_| IngressError::Rejected)?;
        let session = EnrollmentSession {
            local_installation_key: controller_key,
            remote_installation_key: remote_key,
            channel_binding: binding,
        };
        Ok(EnrollmentClient {
            socket: Some(socket),
            transport,
            session,
        })
    })
    .await
    .map_err(|_| IngressError::Unavailable)?
}

impl EnrollmentClient {
    pub async fn request(
        &mut self,
        op: EnrollmentOperation,
        payload: &[u8],
    ) -> Result<Vec<u8>, EnrollmentError> {
        if payload.len() > usize::from(u16::MAX) - 17 {
            return Err(EnrollmentError::InvalidFrame);
        }
        let mut socket = self.socket.take().ok_or(EnrollmentError::Unavailable)?;
        let reply = tokio::time::timeout(Duration::from_secs(60), async {
            let mut frame = vec![match op {
                EnrollmentOperation::Discover => 1,
                EnrollmentOperation::Apply => 2,
                EnrollmentOperation::Status => 3,
            }];
            frame.extend_from_slice(payload);
            let mut ciphertext = vec![0; frame.len() + 16];
            self.transport
                .write_message(&frame, &mut ciphertext)
                .map_err(|_| EnrollmentError::InvalidFrame)?;
            socket
                .send(Message::Binary(ciphertext.into()))
                .await
                .map_err(|_| EnrollmentError::Unavailable)?;
            let response = read(&mut socket)
                .await
                .map_err(|_| EnrollmentError::Unavailable)?;
            let mut plaintext = vec![0; response.len()];
            let len = self
                .transport
                .read_message(&response, &mut plaintext)
                .map_err(|_| EnrollmentError::Rejected)?;
            plaintext.truncate(len);
            Ok::<_, EnrollmentError>(plaintext)
        })
        .await
        .map_err(|_| EnrollmentError::Unavailable)??;
        if op != EnrollmentOperation::Apply {
            self.socket = Some(socket);
        }
        match reply.split_first() {
            Some((0, body)) => Ok(body.to_vec()),
            Some((2, [])) => Err(EnrollmentError::Rejected),
            Some((3, [])) => Err(EnrollmentError::AlreadyApplied),
            Some((4, [])) => Err(EnrollmentError::Unavailable),
            _ => Err(EnrollmentError::InvalidFrame),
        }
    }
}
