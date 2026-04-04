//! Trust management — inverted model inspired by Nebula's CA pool.
//!
//! Trust model (inverted from typical CA):
//! - Default = Trusted. All mesh peers are trusted unless King says otherwise.
//! - King signs Untrusted declarations with Ed25519 + TTL.
//! - Expired Untrusted declaration = peer returns to Trusted (safe default).
//! - King offline + all declarations expired = everyone trusted.
//! - Only Untrusted declarations are stored and synced.

use super::crown::{self, CrownKeyPair, TrustDeclaration, TrustLevel};

/// Default TTL for trust declarations: 5 minutes.
pub const TRUST_TTL_SECS: u64 = 300;

/// Refresh declarations when this many seconds remain before expiry.
pub const TRUST_REFRESH_THRESHOLD_SECS: u64 = 60;

/// Manages trust declarations for the mesh.
///
/// Only Untrusted declarations are meaningful. Absence of a declaration = Trusted.
pub struct TrustManager {
    pub declarations: Vec<TrustDeclaration>,
}

impl TrustManager {
    /// Load declarations from disk and validate against crown key.
    /// Crown holder: re-signs expired Untrusted declarations with fresh TTL.
    /// Non-crown: purges declarations with invalid signatures.
    pub fn load_and_validate(
        crown_keypair: Option<&CrownKeyPair>,
        crown_pubkey: Option<&str>,
        holder_id: &str,
    ) -> Self {
        let mut declarations = crown::load_trust_declarations();

        // Only keep Untrusted declarations — others are meaningless in the inverted model
        declarations.retain(|d| d.trust_level == TrustLevel::Untrusted);

        if let Some(kp) = crown_keypair {
            // Crown holder: re-sign expired Untrusted declarations
            let mut result = Vec::new();
            for decl in &declarations {
                if let Some(pk) = crown_pubkey {
                    if crown::verify_trust_declaration(decl, pk).is_ok() {
                        result.push(decl.clone());
                        continue;
                    }
                    if crown::verify_trust_signature(decl, pk).is_err() {
                        log::warn!("[mesh/trust] Dropping declaration for {} — invalid signature", decl.peer_id);
                        continue;
                    }
                }
                // Re-sign with fresh TTL
                match crown::create_trust_declaration(kp, &decl.peer_id, TrustLevel::Untrusted, holder_id, decl.version + 1) {
                    Ok(new_decl) => {
                        log::info!("[mesh/trust] Re-signed untrust for {} v{}", decl.peer_id, decl.version + 1);
                        result.push(new_decl);
                    }
                    Err(e) => log::warn!("[mesh/trust] Failed to re-sign for {}: {}", decl.peer_id, e),
                }
            }
            if result.len() != declarations.len() || result.iter().any(|d| !declarations.iter().any(|od| od.peer_id == d.peer_id && od.version == d.version)) {
                declarations = result;
                let _ = crown::save_trust_declarations(&declarations);
            }
        } else if let Some(pk) = crown_pubkey {
            // Non-crown: keep only declarations with valid signatures
            let before = declarations.len();
            declarations.retain(|d| crown::verify_trust_signature(d, pk).is_ok());
            if declarations.len() < before {
                log::info!("[mesh/trust] Purged {} invalid declarations", before - declarations.len());
                let _ = crown::save_trust_declarations(&declarations);
            }
        }

        Self { declarations }
    }

    /// Get trust level for a peer.
    /// Crown holder is always Trusted. Default is Trusted.
    /// Only returns Untrusted if there is a valid, non-expired Untrusted declaration.
    pub fn get_trust_level(&self, peer_id: &str, crown_holder_id: Option<&str>) -> TrustLevel {
        if crown_holder_id == Some(peer_id) {
            return TrustLevel::Trusted;
        }
        // Check for a valid Untrusted declaration
        let is_untrusted = self.declarations.iter()
            .any(|d| d.peer_id == peer_id && d.trust_level == TrustLevel::Untrusted && !Self::is_expired(d));
        if is_untrusted {
            TrustLevel::Untrusted
        } else {
            TrustLevel::Trusted
        }
    }

    /// Create a signed trust declaration for a peer (crown holder only).
    pub fn set_trust_level(
        &mut self,
        keypair: &CrownKeyPair,
        peer_id: &str,
        level: TrustLevel,
        holder_id: &str,
    ) -> Result<TrustDeclaration, String> {
        let version = self.declarations.iter()
            .filter(|d| d.peer_id == peer_id)
            .map(|d| d.version)
            .max()
            .unwrap_or(0) + 1;

        if level == TrustLevel::Trusted {
            // Trusted is the default — just remove any Untrusted declaration
            self.declarations.retain(|d| d.peer_id != peer_id);
            let _ = crown::save_trust_declarations(&self.declarations);
            // Return a declaration for the API response, but don't store it
            return crown::create_trust_declaration(keypair, peer_id, level, holder_id, version);
        }

        let decl = crown::create_trust_declaration(keypair, peer_id, level, holder_id, version)?;
        self.declarations.retain(|d| d.peer_id != peer_id);
        self.declarations.push(decl.clone());
        let _ = crown::save_trust_declarations(&self.declarations);
        Ok(decl)
    }

    /// Count trusted peers (for quorum calculation).
    pub fn trusted_peer_count(
        &self,
        paired_peer_ids: impl Iterator<Item = impl AsRef<str>>,
        crown_holder_id: Option<&str>,
    ) -> usize {
        paired_peer_ids
            .filter(|id| self.get_trust_level(id.as_ref(), crown_holder_id) == TrustLevel::Trusted)
            .count()
    }

    /// Merge incoming declarations from sync. Verifies signatures, accepts higher versions.
    /// Only Untrusted declarations are stored.
    pub fn merge_declarations(&mut self, incoming: &[TrustDeclaration], crown_pubkey: &str) {
        let mut changed = false;
        for decl in incoming {
            if crown::verify_trust_signature(decl, crown_pubkey).is_err() {
                log::warn!("[mesh/trust] Rejected declaration for {}: bad signature", decl.peer_id);
                continue;
            }
            // Only store Untrusted declarations
            if decl.trust_level == TrustLevel::Trusted {
                // Incoming Trusted = remove any Untrusted declaration for this peer
                let before = self.declarations.len();
                self.declarations.retain(|d| d.peer_id != decl.peer_id);
                if self.declarations.len() < before {
                    changed = true;
                }
                continue;
            }
            let existing_ver = self.declarations.iter()
                .filter(|d| d.peer_id == decl.peer_id)
                .map(|d| d.version)
                .max()
                .unwrap_or(0);
            if decl.version > existing_ver {
                self.declarations.retain(|d| d.peer_id != decl.peer_id);
                self.declarations.push(decl.clone());
                log::info!("[mesh/trust] Accepted: {} → {:?} v{}", decl.peer_id, decl.trust_level, decl.version);
                changed = true;
            }
        }
        if changed {
            let _ = crown::save_trust_declarations(&self.declarations);
        }
    }

    /// Get valid Untrusted declarations for sync payload.
    pub fn valid_declarations(&self, crown_pubkey: &str) -> Vec<TrustDeclaration> {
        self.declarations.iter()
            .filter(|d| {
                d.trust_level == TrustLevel::Untrusted
                    && !Self::is_expired(d)
                    && crown::verify_trust_declaration(d, crown_pubkey).is_ok()
            })
            .cloned()
            .collect()
    }

    /// Get peer_ids whose Untrusted declarations expire within the refresh threshold.
    pub fn declarations_needing_refresh(&self) -> Vec<String> {
        let now = Self::now_secs();
        self.declarations.iter()
            .filter(|d| d.trust_level == TrustLevel::Untrusted)
            .filter(|d| match d.expires_at {
                Some(exp) => exp > now && exp.saturating_sub(now) <= TRUST_REFRESH_THRESHOLD_SECS,
                None => true,
            })
            .map(|d| d.peer_id.clone())
            .collect()
    }

    /// Purge expired declarations from memory and disk.
    pub fn purge_expired(&mut self) {
        let before = self.declarations.len();
        self.declarations.retain(|d| !Self::is_expired(d));
        if self.declarations.len() < before {
            log::info!("[mesh/trust] Purged {} expired declarations", before - self.declarations.len());
            let _ = crown::save_trust_declarations(&self.declarations);
        }
    }

    /// Remove all declarations for a peer.
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.declarations.retain(|d| d.peer_id != peer_id);
        let _ = crown::save_trust_declarations(&self.declarations);
    }

    /// Clear all declarations.
    pub fn clear(&mut self) {
        self.declarations.clear();
        let _ = crown::save_trust_declarations(&[]);
    }

    /// Save current declarations to disk.
    pub fn save(&self) {
        let _ = crown::save_trust_declarations(&self.declarations);
    }

    fn is_expired(decl: &TrustDeclaration) -> bool {
        match decl.expires_at {
            Some(exp) => Self::now_secs() > exp,
            None => true,
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keypair() -> CrownKeyPair {
        CrownKeyPair::generate()
    }

    #[test]
    fn test_default_is_trusted() {
        let tm = TrustManager { declarations: vec![] };
        // No declarations = Trusted (inverted model)
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Trusted);
        assert_eq!(tm.get_trust_level("unknown-peer", None), TrustLevel::Trusted);
    }

    #[test]
    fn test_untrusted_declaration() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        tm.set_trust_level(&kp, "peer-1", TrustLevel::Untrusted, "crown-holder").unwrap();
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Untrusted);
    }

    #[test]
    fn test_expired_untrusted_returns_to_trusted() {
        let kp = make_keypair();
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown-holder", 1).unwrap();
        decl.expires_at = Some(1); // Expired
        let tm = TrustManager { declarations: vec![decl] };
        // Expired Untrusted → falls back to Trusted
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Trusted);
    }

    #[test]
    fn test_crown_holder_always_trusted() {
        let tm = TrustManager { declarations: vec![] };
        assert_eq!(tm.get_trust_level("king", Some("king")), TrustLevel::Trusted);
    }

    #[test]
    fn test_set_trusted_removes_untrust() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        tm.set_trust_level(&kp, "peer-1", TrustLevel::Untrusted, "crown").unwrap();
        assert_eq!(tm.declarations.len(), 1);
        tm.set_trust_level(&kp, "peer-1", TrustLevel::Trusted, "crown").unwrap();
        // Setting Trusted removes the declaration (trusted is default)
        assert_eq!(tm.declarations.len(), 0);
        assert_eq!(tm.get_trust_level("peer-1", Some("crown")), TrustLevel::Trusted);
    }

    #[test]
    fn test_declarations_needing_refresh() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let now = TrustManager::now_secs();
        // Untrusted declaration expiring soon
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        decl.expires_at = Some(now + 30);
        tm.declarations.push(decl);
        // Untrusted declaration with plenty of time
        let mut decl2 = crown::create_trust_declaration(&kp, "peer-2", TrustLevel::Untrusted, "crown", 1).unwrap();
        decl2.expires_at = Some(now + 200);
        tm.declarations.push(decl2);

        let needing = tm.declarations_needing_refresh();
        assert!(needing.contains(&"peer-1".to_string()));
        assert!(!needing.contains(&"peer-2".to_string()));
    }

    #[test]
    fn test_purge_expired() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        decl.expires_at = Some(1); // Expired
        tm.declarations.push(decl);
        let decl2 = crown::create_trust_declaration(&kp, "peer-2", TrustLevel::Untrusted, "crown", 1).unwrap();
        tm.declarations.push(decl2);

        tm.purge_expired();
        assert_eq!(tm.declarations.len(), 1);
        assert_eq!(tm.declarations[0].peer_id, "peer-2");
    }

    #[test]
    fn test_merge_declarations() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        tm.merge_declarations(&[decl], &kp.public_key);
        assert_eq!(tm.declarations.len(), 1);

        // Higher version replaces
        let decl2 = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 2).unwrap();
        tm.merge_declarations(&[decl2], &kp.public_key);
        assert_eq!(tm.declarations.len(), 1);
        assert_eq!(tm.declarations[0].version, 2);

        // Lower version rejected
        let decl3 = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        tm.merge_declarations(&[decl3], &kp.public_key);
        assert_eq!(tm.declarations[0].version, 2);
    }

    #[test]
    fn test_merge_trusted_removes_untrust() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        tm.merge_declarations(&[decl], &kp.public_key);
        assert_eq!(tm.declarations.len(), 1);

        // Merging a Trusted declaration removes the Untrusted one
        let trusted_decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown", 2).unwrap();
        tm.merge_declarations(&[trusted_decl], &kp.public_key);
        assert_eq!(tm.declarations.len(), 0);
    }
}
