use crate::wire::{
    decode_url, EncryptionKeyIds, Envelope, SignedEnvelope, MAX_CIPHERTEXT_BYTES, SCHEMA,
};
use crate::{Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand::{CryptoRng, RngCore};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const X25519_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
];
const ED25519_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct PrivateKeys {
    encryption: [u8; 32],
    signing: [u8; 32],
}

impl PrivateKeys {
    pub fn generate(rng: &mut (impl RngCore + CryptoRng)) -> Self {
        let mut keys = Self {
            encryption: [0; 32],
            signing: [0; 32],
        };
        rng.fill_bytes(&mut keys.encryption);
        rng.fill_bytes(&mut keys.signing);
        keys
    }

    pub(crate) fn from_storage(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 64 {
            return Err(Error::SecretStore);
        }
        let mut keys = Self {
            encryption: [0; 32],
            signing: [0; 32],
        };
        keys.encryption.copy_from_slice(&bytes[..32]);
        keys.signing.copy_from_slice(&bytes[32..]);
        Ok(keys)
    }

    pub(crate) fn storage_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(64));
        bytes.extend_from_slice(&self.encryption);
        bytes.extend_from_slice(&self.signing);
        bytes
    }

    pub fn encryption_spki(&self) -> String {
        let public = PublicKey::from(&StaticSecret::from(self.encryption));
        spki(&X25519_PREFIX, public.as_bytes())
    }

    pub fn signing_spki(&self) -> String {
        let public = SigningKey::from_bytes(&self.signing).verifying_key();
        spki(&ED25519_PREFIX, public.as_bytes())
    }

    pub fn encryption_key_id(&self) -> String {
        encryption_key_id(&self.encryption_spki())
    }

    pub fn verifier_key_id(&self, uid: &str) -> String {
        format!(
            "vk_{}",
            &sha256_hex(format!("verifier-key:{uid}:{}", self.signing_spki()).as_bytes())[..32]
        )
    }

    pub fn sign(&self, envelope: Envelope) -> Result<SignedEnvelope> {
        let bytes = envelope.canonical_bytes()?;
        let signature = SigningKey::from_bytes(&self.signing).sign(&bytes);
        Ok(SignedEnvelope {
            envelope,
            signature: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        })
    }

    pub fn encrypt(
        &self,
        envelope: &mut Envelope,
        peer_spki: &str,
        body: &str,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<()> {
        let mut nonce = [0; 12];
        rng.fill_bytes(&mut nonce);
        self.encrypt_with_nonce(envelope, peer_spki, body, nonce)
    }

    fn encrypt_with_nonce(
        &self,
        envelope: &mut Envelope,
        peer_spki: &str,
        body: &str,
        nonce: [u8; 12],
    ) -> Result<()> {
        crate::wire::validate_message_size(body)?;
        let ids = EncryptionKeyIds {
            sender: self.encryption_key_id(),
            recipient: encryption_key_id(peer_spki),
        };
        let key = self.shared_key(peer_spki, &envelope.friendship_id, &ids)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| Error::InvalidKey)?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), body.as_bytes())
            .map_err(|_| Error::Decryption)?;
        envelope.ciphertext = URL_SAFE_NO_PAD.encode(ciphertext);
        envelope.nonce = URL_SAFE_NO_PAD.encode(nonce);
        envelope.enc_key_ids = ids;
        envelope.validate()
    }

    pub(crate) fn decrypt(
        &self,
        envelope: &Envelope,
        peer_spki: &str,
    ) -> Result<Zeroizing<String>> {
        envelope.validate()?;
        if envelope.enc_key_ids.recipient != self.encryption_key_id()
            || envelope.enc_key_ids.sender != encryption_key_id(peer_spki)
        {
            return Err(Error::InvalidKey);
        }
        let key = self.shared_key(peer_spki, &envelope.friendship_id, &envelope.enc_key_ids)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| Error::InvalidKey)?;
        let nonce = decode_url(&envelope.nonce, 12, 12)?;
        let ciphertext = decode_url(&envelope.ciphertext, 16, MAX_CIPHERTEXT_BYTES)?;
        let mut bytes = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_slice())
            .map_err(|_| Error::Decryption)?;
        if std::str::from_utf8(&bytes).is_err() {
            bytes.zeroize();
            return Err(Error::Decryption);
        }
        String::from_utf8(bytes)
            .map(Zeroizing::new)
            .map_err(|_| Error::Decryption)
    }

    fn shared_key(
        &self,
        peer_spki: &str,
        friendship: &str,
        ids: &EncryptionKeyIds,
    ) -> Result<Zeroizing<[u8; 32]>> {
        let peer = PublicKey::from(parse_spki(peer_spki, &X25519_PREFIX)?);
        let shared = StaticSecret::from(self.encryption).diffie_hellman(&peer);
        if !shared.was_contributory() {
            return Err(Error::InvalidKey);
        }
        let info = serde_json::to_vec(&[SCHEMA, friendship, &ids.sender, &ids.recipient])
            .map_err(|_| Error::InvalidEnvelope)?;
        let mut key = Zeroizing::new([0u8; 32]);
        Hkdf::<Sha256>::new(None, shared.as_bytes())
            .expand(&info, key.as_mut())
            .map_err(|_| Error::InvalidKey)?;
        Ok(key)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn encryption_key_id(spki: &str) -> String {
    format!("ek_{}", &sha256_hex(spki.as_bytes())[..32])
}

pub fn validate_encryption_spki(spki: &str) -> Result<()> {
    parse_spki(spki, &X25519_PREFIX).map(|_| ())
}

pub fn verify(envelope: &Envelope, signature: &str, public_spki: &str) -> Result<()> {
    let bytes = envelope.canonical_bytes()?;
    let raw = decode_url(signature, 64, 64).map_err(|_| Error::BadSignature)?;
    let signature = Signature::from_slice(&raw).map_err(|_| Error::BadSignature)?;
    let public = VerifyingKey::from_bytes(&parse_spki(public_spki, &ED25519_PREFIX)?)
        .map_err(|_| Error::InvalidKey)?;
    public
        .verify_strict(&bytes, &signature)
        .map_err(|_| Error::BadSignature)
}

fn spki(prefix: &[u8; 12], public: &[u8; 32]) -> String {
    let mut der = [0; 44];
    der[..12].copy_from_slice(prefix);
    der[12..].copy_from_slice(public);
    STANDARD.encode(der)
}

fn parse_spki(value: &str, prefix: &[u8; 12]) -> Result<[u8; 32]> {
    if value.len() != 60 {
        return Err(Error::InvalidKey);
    }
    let der = STANDARD.decode(value).map_err(|_| Error::InvalidKey)?;
    if der.len() != 44 || der[..12] != *prefix || STANDARD.encode(&der) != value {
        return Err(Error::InvalidKey);
    }
    der[12..].try_into().map_err(|_| Error::InvalidKey)
}

#[cfg(test)]
pub(crate) mod tests;
