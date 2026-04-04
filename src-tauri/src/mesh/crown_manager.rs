//! Crown lifecycle — claiming, transferring, accepting, updating crown ownership.
//! Separated from crown.rs (crypto primitives) for SRP.

use std::collections::HashMap;
use super::crown::{self, CrownKeyPair, CrownPublicInfo, CrownClaims, EmergencyVote};
use super::types::PairedPeer;

/// Manages crown (leadership) state for this instance.
pub struct CrownManager {
    /// Crown keypair — only present if this instance is the crown holder.
    pub keypair: Option<CrownKeyPair>,
    /// Public crown info — known to all peers, used to verify crown commands.
    pub public_info: Option<CrownPublicInfo>,
    /// Emergency election votes (pending).
    pub emergency_votes: Vec<EmergencyVote>,
}

impl CrownManager {
    pub fn new() -> Self {
        let keypair = crown::load_crown_keypair();
        let public_info = crown::load_crown_public_info();
        log::info!("[mesh/crown] Loaded: holder={}", keypair.is_some());
        Self { keypair, public_info, emergency_votes: Vec::new() }
    }

    /// Whether this instance is the crown holder.
    pub fn is_crown_holder(&self) -> bool {
        self.keypair.is_some()
    }

    /// Get the crown holder's instance ID (as known by this peer).
    pub fn crown_holder_id(&self) -> Option<String> {
        self.public_info.as_ref().map(|c| c.holder_id.clone())
    }

    /// Claim crown if nobody has it yet.
    /// Skips if we already have a crown (ours or received from another mesh).
    pub fn claim_if_unclaimed(&mut self, our_id: &str) {
        if self.keypair.is_some() || self.public_info.is_some() {
            return;
        }
        log::info!("[mesh/crown] No crown — claiming");
        let keypair = CrownKeyPair::generate();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mesh_id = uuid::Uuid::new_v4().to_string();
        let claims = CrownClaims {
            holder_id: our_id.to_string(),
            mesh_id,
            crowned_at: now,
            version: keypair.version,
            emergency: false,
            voted_by: vec![],
            iat: now,
        };
        match crown::create_crown_jwt(&keypair, &claims) {
            Ok(jwt) => {
                let public_info = CrownPublicInfo {
                    public_key: keypair.public_key.clone(),
                    crown_jwt: jwt,
                    version: keypair.version,
                    holder_id: our_id.to_string(),
                };
                let _ = crown::save_crown_keypair(&keypair);
                let _ = crown::save_crown_public_info(&public_info);
                self.keypair = Some(keypair);
                self.public_info = Some(public_info);
                log::info!("[mesh/crown] Claimed crown — this instance is now the leader");
            }
            Err(e) => log::error!("[mesh/crown] Failed to create crown JWT: {}", e),
        }
    }

    /// Transfer crown to a paired peer. Only crown holder can call this.
    pub fn transfer(&mut self, target_id: &str, paired_peers: &HashMap<String, PairedPeer>) -> Result<u64, String> {
        let keypair = self.keypair.as_ref()
            .ok_or("Only the crown holder can transfer the crown")?;
        if !paired_peers.contains_key(target_id) {
            return Err("Peer not found".to_string());
        }
        let next_version = keypair.version + 1;
        crown::delete_crown_keypair();
        self.keypair = None;
        log::info!("[mesh/crown] Crown relinquished, transferring to {} (v{})", target_id, next_version);
        Ok(next_version)
    }

    /// Accept crown transfer — generate keypair locally, return public info for broadcast.
    pub fn accept(
        &mut self,
        version: u64,
        from_id: &str,
        our_id: &str,
        paired_peers: &HashMap<String, PairedPeer>,
    ) -> Result<CrownPublicInfo, String> {
        if !paired_peers.contains_key(from_id) {
            return Err(format!("Unknown sender: {}", from_id));
        }
        let mut keypair = CrownKeyPair::generate();
        keypair.version = version;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let mesh_id = self.public_info.as_ref()
            .and_then(|c| crown::verify_crown_jwt(&c.crown_jwt, &c.public_key).ok())
            .map(|cl| cl.mesh_id)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let claims = CrownClaims {
            holder_id: our_id.to_string(),
            mesh_id, crowned_at: now, version,
            emergency: false, voted_by: vec![], iat: now,
        };
        let jwt = crown::create_crown_jwt(&keypair, &claims)?;
        let public_info = CrownPublicInfo {
            public_key: keypair.public_key.clone(), crown_jwt: jwt,
            version, holder_id: our_id.to_string(),
        };
        let _ = crown::save_crown_keypair(&keypair);
        let _ = crown::save_crown_public_info(&public_info);
        self.keypair = Some(keypair);
        self.public_info = Some(public_info.clone());
        log::info!("[mesh/crown] Accepted crown from {} — this instance is now leader (v{})", from_id, version);
        Ok(public_info)
    }

    /// Update crown public info (called on all peers when crown transfers).
    pub fn update_public(&mut self, public_info: CrownPublicInfo, our_id: &str) -> Result<(), String> {
        let verified_claims = crown::verify_crown_jwt(&public_info.crown_jwt, &public_info.public_key)
            .map_err(|e| format!("Crown JWT invalid: {}", e))?;

        let verified_holder = &verified_claims.holder_id;
        log::info!("[mesh/crown] Crown info: holder={}, v{}", verified_holder, public_info.version);

        if let Some(ref existing) = self.public_info {
            if existing.holder_id == *verified_holder {
                if public_info.version <= existing.version {
                    return Err(format!("Stale crown: have v{}, got v{}", existing.version, public_info.version));
                }
            } else {
                if public_info.version <= existing.version {
                    return Err(format!("Stale crown: have v{} from {}, got v{} from {}",
                        existing.version, existing.holder_id, public_info.version, verified_holder));
                }
                if public_info.version != existing.version + 1 {
                    return Err(format!("Non-sequential crown: have v{}, got v{}", existing.version, public_info.version));
                }
            }
        }

        if *verified_holder != our_id && self.keypair.is_some() {
            log::info!("[mesh/crown] Relinquishing crown to {} v{}", verified_holder, public_info.version);
            crown::delete_crown_keypair();
            self.keypair = None;
        }

        let _ = crown::save_crown_public_info(&public_info);
        self.public_info = Some(public_info);
        Ok(())
    }

    /// Get the mesh_id from the current crown (if any).
    pub fn mesh_id(&self) -> Option<String> {
        self.public_info.as_ref().and_then(|c| {
            crown::verify_crown_jwt(&c.crown_jwt, &c.public_key)
                .ok()
                .map(|claims| claims.mesh_id)
        })
    }

    /// Strip our own crown when joining a foreign mesh.
    /// Called when we receive a crown_public with a different mesh_id.
    pub fn strip_foreign_crown(&mut self, incoming: &CrownPublicInfo) {
        let incoming_mesh_id = crown::verify_crown_jwt(&incoming.crown_jwt, &incoming.public_key)
            .ok()
            .map(|c| c.mesh_id);

        let our_mesh_id = self.mesh_id();

        if our_mesh_id.is_some() && our_mesh_id != incoming_mesh_id {
            log::info!(
                "[mesh/crown] Stripping own crown (mesh={:?}) — joining foreign mesh (mesh={:?})",
                our_mesh_id, incoming_mesh_id
            );
            if self.keypair.is_some() {
                crown::delete_crown_keypair();
                self.keypair = None;
            }
            // Accept the foreign crown
            let _ = crown::save_crown_public_info(incoming);
            self.public_info = Some(incoming.clone());
        }
    }

    /// Relinquish crown and clear state.
    pub fn relinquish(&mut self) {
        if self.keypair.is_some() {
            crown::delete_crown_keypair();
            self.keypair = None;
        }
        self.public_info = None;
    }
}
