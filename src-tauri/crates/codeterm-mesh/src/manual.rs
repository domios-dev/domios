use std::net::SocketAddr;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use serde::{Deserialize, Serialize};

use crate::{
    discovered::DiscoverySighting, types::DiscoveryMethod, types::PeerAddress, MeshManager,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityChallenge {
    pub address: SocketAddr,
    pub nonce: uuid::Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerIdentity {
    pub challenge: IdentityChallenge,
    pub instance_id: String,
    pub instance_name: String,
    pub public_key: String,
    pub dev: bool,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub mesh_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityProof {
    pub identity: PeerIdentity,
    pub signature: String,
}

impl IdentityProof {
    pub fn from_manager(mesh: &MeshManager, challenge: IdentityChallenge) -> Result<Self, String> {
        let addresses = crate::discovery::remote_addresses(mesh)?;
        if challenge.address.port() != mesh.api_port
            || !addresses
                .iter()
                .any(|a| a.ip == challenge.address.ip().to_string())
        {
            return Err("mesh_identity_address_not_ready".into());
        }
        let private_key = mesh
            .ident
            .identity
            .private_key
            .as_deref()
            .ok_or("No identity keypair")?;
        let identity = PeerIdentity {
            challenge,
            instance_id: mesh.ident.identity.instance_id.clone(),
            instance_name: mesh.ident.instance_name.clone(),
            public_key: crate::crown::public_key_from_private(private_key)?,
            dev: codeterm_paths::is_dev_instance(),
            addresses,
            api_port: mesh.api_port,
            mesh_id: if mesh.paired_peers.is_empty() {
                None
            } else {
                Some(mesh.ident.identity.instance_id.clone())
            },
        };
        Self::sign(identity, private_key)
    }

    pub fn sign(identity: PeerIdentity, private_key: &str) -> Result<Self, String> {
        let bytes: [u8; 32] = B64
            .decode(private_key)
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "Invalid private key length")?;
        let payload = serde_json::to_vec(&identity).map_err(|e| e.to_string())?;
        let signature = SigningKey::from_bytes(&bytes).sign(&payload);
        Ok(Self {
            identity,
            signature: B64.encode(signature.to_bytes()),
        })
    }

    pub fn verify(
        &self,
        challenge: &IdentityChallenge,
        target: Option<&str>,
    ) -> Result<DiscoverySighting, String> {
        let identity = &self.identity;
        if identity.challenge.address != challenge.address
            || identity.challenge.nonce != challenge.nonce
        {
            return Err("mesh_identity_challenge_mismatch".into());
        }
        let key = crate::crown::verifying_key_from_b64(&identity.public_key)?;
        let signature =
            Signature::from_slice(&B64.decode(&self.signature).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        key.verify(
            &serde_json::to_vec(identity).map_err(|e| e.to_string())?,
            &signature,
        )
        .map_err(|_| "mesh_identity_signature_invalid")?;
        uuid::Uuid::parse_str(&identity.instance_id).map_err(|_| "mesh_identity_id_invalid")?;
        if let Some(target) = target {
            let is_id = uuid::Uuid::parse_str(target).is_ok()
                || (target.len() >= 6 && target.bytes().all(|b| b.is_ascii_hexdigit()));
            let matches = if is_id {
                identity.instance_id == target || identity.instance_id.starts_with(target)
            } else {
                identity.instance_id == target || identity.instance_name == target
            };
            if !matches {
                return Err(format!(
                    "mesh_identity_mismatch: requested {target}, answered {}",
                    identity.instance_id
                ));
            }
        }
        let address = PeerAddress::from_ip(&challenge.address.ip().to_string());
        if identity.api_port != challenge.address.port()
            || !identity.addresses.iter().any(|a| a.ip == address.ip)
        {
            return Err("mesh_identity_address_not_ready".into());
        }
        let mut addresses = vec![address];
        for candidate in &identity.addresses {
            if let Ok(ip) = candidate.ip.parse::<std::net::IpAddr>() {
                if !addresses.iter().any(|a| a.ip == ip.to_string()) {
                    addresses.push(PeerAddress::from_ip(&ip.to_string()));
                }
            }
        }
        Ok(DiscoverySighting {
            instance_id: identity.instance_id.clone(),
            instance_name: identity.instance_name.clone(),
            addresses,
            api_port: identity.api_port,
            method: DiscoveryMethod::Manual,
            dev: identity.dev,
            mesh_id: identity.mesh_id.clone(),
            voucher: None,
        })
    }
}

pub fn register_identity(
    mesh: &mut MeshManager,
    proof: &IdentityProof,
    challenge: &IdentityChallenge,
    target: Option<&str>,
    now: u64,
) -> Result<String, String> {
    let sighting = proof.verify(challenge, target)?;
    let id = sighting.instance_id.clone();
    if !mesh.upsert_discovered(sighting, now) {
        return Err("mesh_identity_registration_refused: own, blocked or revoked identity".into());
    }
    Ok(id)
}

pub async fn read_identity(
    address: SocketAddr,
    target: Option<&str>,
) -> Result<(IdentityProof, IdentityChallenge), String> {
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        return Err("Expected a concrete IP address and nonzero port".into());
    }
    let challenge = IdentityChallenge {
        address,
        nonce: uuid::Uuid::new_v4(),
    };
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("http://{address}/api/mesh/pair/identity"))
        .json(&challenge)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Peer identity read failed: HTTP {}",
            response.status()
        ));
    }
    let proof: IdentityProof = response.json().await.map_err(|e| e.to_string())?;
    proof.verify(&challenge, target)?;
    Ok((proof, challenge))
}

pub async fn discover_at(
    mesh: &crate::MeshState,
    address: SocketAddr,
    target: Option<&str>,
) -> Result<String, String> {
    let (proof, challenge) = read_identity(address, target).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    register_identity(&mut mesh.lock(), &proof, &challenge, target, now)
}

#[cfg(test)]
mod tests;
