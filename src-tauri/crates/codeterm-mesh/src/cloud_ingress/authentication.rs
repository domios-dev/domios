use std::net::SocketAddr;

use super::EnrollmentSession;
use ed25519_dalek::{Signature, VerifyingKey};
use snow::Builder;

pub struct IngressReady {
    pub(super) bound_address: SocketAddr,
}

impl IngressReady {
    pub fn bound_address(&self) -> SocketAddr {
        self.bound_address
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngressError {
    Rejected,
    InvalidFrame,
    Unavailable,
    TlsFailed,
}

impl IngressError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Rejected => "ingress_rejected",
            Self::InvalidFrame => "invalid_frame",
            Self::Unavailable => "ingress_unavailable",
            Self::TlsFailed => "ingress_tls_failed",
        }
    }
}

pub struct ChannelProof {
    message: Vec<u8>,
}

impl ChannelProof {
    pub(super) fn bootstrap(domain: &[u8], binding: &[u8; 32], key: &[u8; 32]) -> Self {
        let mut message = domain.to_vec();
        message.extend_from_slice(binding);
        message.extend_from_slice(key);
        Self { message }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.message
    }
}

pub struct BootstrapResponder {
    handshake: snow::HandshakeState,
}

impl BootstrapResponder {
    pub fn new(psk: &[u8; 32]) -> Result<Self, IngressError> {
        let params = "Noise_NNpsk0_25519_ChaChaPoly_SHA256"
            .parse()
            .map_err(|_| IngressError::Unavailable)?;
        let handshake = Builder::new(params)
            .prologue(b"domios-cloud-ingress-bootstrap-v1")
            .and_then(|builder| builder.psk(0, psk))
            .and_then(Builder::build_responder)
            .map_err(|_| IngressError::Unavailable)?;
        Ok(Self { handshake })
    }

    pub(super) fn respond(self, offer: &[u8]) -> Result<BootstrapHandshake, IngressError> {
        let mut handshake = self.handshake;
        if offer.len() != 48 || handshake.read_message(offer, &mut []).is_err() {
            return Err(IngressError::Rejected);
        }
        let mut reply = [0; 48];
        let len = handshake
            .write_message(&[], &mut reply)
            .map_err(|_| IngressError::Unavailable)?;
        if len != reply.len() {
            return Err(IngressError::InvalidFrame);
        }
        let binding = handshake
            .get_handshake_hash()
            .try_into()
            .map_err(|_| IngressError::Unavailable)?;
        let transport = handshake
            .into_transport_mode()
            .map_err(|_| IngressError::Unavailable)?;
        Ok(BootstrapHandshake {
            reply,
            binding,
            transport,
        })
    }
}

pub(super) struct BootstrapHandshake {
    pub reply: [u8; 48],
    binding: [u8; 32],
    pub(super) transport: snow::TransportState,
}

impl BootstrapHandshake {
    pub(super) fn responder_proof(
        &self,
        authority: &dyn CloudIngressAuthority,
        session: &EnrollmentSession,
    ) -> Result<[u8; 96], IngressError> {
        let key = session.local_installation_key();
        let proof = ChannelProof::bootstrap(
            b"domios-cloud-ingress-bootstrap-responder-v1\0",
            &self.binding,
            key,
        );
        let mut frame = [0; 96];
        frame[..32].copy_from_slice(key);
        frame[32..].copy_from_slice(&authority.sign_channel_proof(&proof)?);
        Ok(frame)
    }

    pub fn authenticate_controller(
        &mut self,
        authority: &dyn CloudIngressAuthority,
        ciphertext: &[u8],
    ) -> Result<EnrollmentSession, IngressError> {
        if ciphertext.len() != 112 {
            return Err(IngressError::InvalidFrame);
        }
        let mut frame = [0u8; 96];
        let len = self
            .transport
            .read_message(ciphertext, &mut frame)
            .map_err(|_| IngressError::Rejected)?;
        if len != frame.len() {
            return Err(IngressError::InvalidFrame);
        }
        let key = frame[..32]
            .try_into()
            .map_err(|_| IngressError::InvalidFrame)?;
        let signature = frame[32..]
            .try_into()
            .map_err(|_| IngressError::InvalidFrame)?;
        let proof = ChannelProof::bootstrap(
            b"domios-cloud-ingress-bootstrap-controller-v1\0",
            &self.binding,
            &key,
        );
        VerifyingKey::from_bytes(&key)
            .map_err(|_| IngressError::Rejected)?
            .verify_strict(proof.as_bytes(), &Signature::from_bytes(&signature))
            .map_err(|_| IngressError::Rejected)?;
        authority.authorize_bootstrap_controller(&key, &self.binding)?;
        Ok(EnrollmentSession {
            remote_installation_key: key,
            local_installation_key: authority.installation_public_key()?,
            channel_binding: self.binding,
        })
    }
}

pub trait CloudIngressAuthority: Send + Sync {
    fn bootstrap_responder(&self) -> Result<BootstrapResponder, IngressError>;
    fn installation_public_key(&self) -> Result<[u8; 32], IngressError>;
    fn sign_channel_proof(&self, proof: &ChannelProof) -> Result<[u8; 64], IngressError>;
    fn authorize_bootstrap_controller(
        &self,
        key: &[u8; 32],
        binding: &[u8; 32],
    ) -> Result<(), IngressError>;
}
