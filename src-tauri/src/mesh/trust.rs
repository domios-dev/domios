//! Trust management — TTL-based trust declarations with centralized King authority.
//!
//! Trust model:
//! - King (crown holder) signs time-limited declarations (Ed25519 + TTL)
//! - Peers verify signature + check expiry locally
//! - Expired declarations → Untrusted (safe default)
//! - Peers refresh declarations from King before expiry
//! - King offline + expired = fallback to Untrusted

use super::crown::{self, CrownKeyPair, TrustDeclaration, TrustLevel};

/// Default TTL for trust declarations: 5 minutes.
pub const TRUST_TTL_SECS: u64 = 300;

/// Refresh declarations when this many seconds remain before expiry.
pub const TRUST_REFRESH_THRESHOLD_SECS: u64 = 60;

/// Manages trust declarations for the mesh.
pub struct TrustManager {
    pub declarations: Vec<TrustDeclaration>,
}

impl TrustManager {
    /// Load declarations from disk and validate against crown key.
    /// Crown holder: re-signs stale/expired declarations.
    /// Non-crown: purges declarations that don't verify.
    pub fn load_and_validate(
        crown_keypair: Option<&CrownKeyPair>,
        crown_pubkey: Option<&str>,
        holder_id: &str,
    ) -> Self {
        let mut declarations = crown::load_trust_declarations();

        if let Some(kp) = crown_keypair {
            // Crown holder: re-sign expired or invalid declarations with fresh TTL
            let mut re_signed = Vec::new();
            for decl in &declarations {
                if let Some(pk) = crown_pubkey {
                    // Use signature-only check (not TTL) — we'll re-sign expired ones
                    if crown::verify_trust_declaration(decl, pk).is_ok() {
                        re_signed.push(decl.clone());
                        continue;
                    }
                    // Signature valid but expired? Re-sign with fresh TTL
                    if crown::verify_trust_signature(decl, pk).is_ok() {
                        // Fall through to re-sign
                    } else {
                        // Truly invalid signature — skip
                        log::warn!("[mesh/trust] Dropping declaration for {} — invalid signature", decl.peer_id);
                        continue;
                    }
                }
                // Re-sign with fresh TTL
                match crown::create_trust_declaration(kp, &decl.peer_id, decl.trust_level.clone(), holder_id, decl.version + 1) {
                    Ok(new_decl) => {
                        log::info!("[mesh/trust] Re-signed declaration for {} v{}", decl.peer_id, decl.version + 1);
                        re_signed.push(new_decl);
                    }
                    Err(e) => log::warn!("[mesh/trust] Failed to re-sign for {}: {}", decl.peer_id, e),
                }
            }
            if re_signed.len() != declarations.len() || re_signed.iter().any(|d| !declarations.iter().any(|od| od.peer_id == d.peer_id && od.version == d.version)) {
                declarations = re_signed;
                let _ = crown::save_trust_declarations(&declarations);
            }
        } else if let Some(pk) = crown_pubkey {
            // Non-crown: keep declarations with valid signatures (even if expired —
            // they'll be refreshed from King; purging them loses version info)
            let before = declarations.len();
            declarations.retain(|d| crown::verify_trust_signature(d, pk).is_ok());
            if declarations.len() < before {
                log::info!("[mesh/trust] Purged {} invalid declarations", before - declarations.len());
                let _ = crown::save_trust_declarations(&declarations);
            }
        }

        Self { declarations }
    }

    /// Get trust level for a peer. Checks signature validity and TTL.
    /// Crown holder is always Trusted (hardcoded).
    pub fn get_trust_level(&self, peer_id: &str, crown_holder_id: Option<&str>) -> TrustLevel {
        // Crown holder is always trusted
        if crown_holder_id == Some(peer_id) {
            return TrustLevel::Trusted;
        }
        self.declarations.iter()
            .filter(|d| d.peer_id == peer_id && !Self::is_expired(d))
            .max_by_key(|d| d.version)
            .map(|d| d.trust_level.clone())
            .unwrap_or(TrustLevel::Untrusted)
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
        let decl = crown::create_trust_declaration(keypair, peer_id, level, holder_id, version)?;
        self.declarations.retain(|d| d.peer_id != peer_id);
        self.declarations.push(decl.clone());
        let _ = crown::save_trust_declarations(&self.declarations);
        Ok(decl)
    }

    /// Auto-trust a peer if crown holder. Returns declaration if created.
    pub fn auto_trust_peer(
        &mut self,
        keypair: Option<&CrownKeyPair>,
        peer_id: &str,
        holder_id: &str,
        crown_holder_id: Option<&str>,
    ) -> Option<TrustDeclaration> {
        let kp = keypair?;
        if self.get_trust_level(peer_id, crown_holder_id) == TrustLevel::Trusted {
            return None;
        }
        match self.set_trust_level(kp, peer_id, TrustLevel::Trusted, holder_id) {
            Ok(decl) => {
                log::info!("[mesh/trust] Auto-trusted peer {} (crown propagation)", peer_id);
                Some(decl)
            }
            Err(e) => {
                log::warn!("[mesh/trust] Failed to auto-trust {}: {}", peer_id, e);
                None
            }
        }
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

    /// Merge incoming declarations from sync. Verifies signatures and accepts higher versions.
    pub fn merge_declarations(&mut self, incoming: &[TrustDeclaration], crown_pubkey: &str) {
        let mut changed = false;
        for decl in incoming {
            if crown::verify_trust_signature(decl, crown_pubkey).is_err() {
                log::warn!("[mesh/trust] Rejected declaration for {}: bad signature", decl.peer_id);
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
                log::info!("[mesh/trust] Accepted declaration: {} → {:?} v{}", decl.peer_id, decl.trust_level, decl.version);
                changed = true;
            }
        }
        if changed {
            let _ = crown::save_trust_declarations(&self.declarations);
        }
    }

    /// Get declarations that are valid (not expired) for sync payload.
    pub fn valid_declarations(&self, crown_pubkey: &str) -> Vec<TrustDeclaration> {
        self.declarations.iter()
            .filter(|d| {
                !Self::is_expired(d)
                    && crown::verify_trust_declaration(d, crown_pubkey).is_ok()
            })
            .cloned()
            .collect()
    }

    /// Get peer_ids whose declarations expire within the refresh threshold.
    pub fn declarations_needing_refresh(&self) -> Vec<String> {
        let now = Self::now_secs();
        self.declarations.iter()
            .filter(|d| {
                match d.expires_at {
                    Some(exp) => exp > now && exp.saturating_sub(now) <= TRUST_REFRESH_THRESHOLD_SECS,
                    None => true, // Legacy declarations always need refresh
                }
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

    /// Check if a declaration is expired.
    fn is_expired(decl: &TrustDeclaration) -> bool {
        match decl.expires_at {
            Some(exp) => Self::now_secs() > exp,
            None => true, // Legacy declarations without TTL are treated as expired
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
    fn test_trust_ttl_valid() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let decl = tm.set_trust_level(&kp, "peer-1", TrustLevel::Trusted, "crown-holder").unwrap();
        assert!(decl.expires_at.is_some());
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Trusted);
    }

    #[test]
    fn test_trust_ttl_expired() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown-holder", 1).unwrap();
        // Force expiry in the past
        decl.expires_at = Some(1);
        tm.declarations.push(decl);
        // Expired declaration → Untrusted
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Untrusted);
    }

    #[test]
    fn test_trust_no_ttl_treated_as_expired() {
        let kp = make_keypair();
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown-holder", 1).unwrap();
        decl.expires_at = None; // Legacy
        let tm = TrustManager { declarations: vec![decl] };
        assert_eq!(tm.get_trust_level("peer-1", Some("crown-holder")), TrustLevel::Untrusted);
    }

    #[test]
    fn test_crown_holder_always_trusted() {
        let tm = TrustManager { declarations: vec![] };
        assert_eq!(tm.get_trust_level("king", Some("king")), TrustLevel::Trusted);
    }

    #[test]
    fn test_declarations_needing_refresh() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        // Declaration expiring within threshold
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown", 1).unwrap();
        let now = TrustManager::now_secs();
        decl.expires_at = Some(now + 30); // 30s left, within threshold
        tm.declarations.push(decl);
        // Declaration with plenty of time
        let mut decl2 = crown::create_trust_declaration(&kp, "peer-2", TrustLevel::Trusted, "crown", 1).unwrap();
        decl2.expires_at = Some(now + 200); // 200s left, fine
        tm.declarations.push(decl2);

        let needing = tm.declarations_needing_refresh();
        assert!(needing.contains(&"peer-1".to_string()));
        assert!(!needing.contains(&"peer-2".to_string()));
    }

    #[test]
    fn test_purge_expired() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let mut decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown", 1).unwrap();
        decl.expires_at = Some(1); // Expired
        tm.declarations.push(decl);
        let decl2 = crown::create_trust_declaration(&kp, "peer-2", TrustLevel::Trusted, "crown", 1).unwrap();
        tm.declarations.push(decl2);

        tm.purge_expired();
        assert_eq!(tm.declarations.len(), 1);
        assert_eq!(tm.declarations[0].peer_id, "peer-2");
    }

    #[test]
    fn test_merge_declarations() {
        let kp = make_keypair();
        let mut tm = TrustManager { declarations: vec![] };
        let decl = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Trusted, "crown", 1).unwrap();
        tm.merge_declarations(&[decl], &kp.public_key);
        assert_eq!(tm.declarations.len(), 1);

        // Higher version replaces
        let decl2 = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Observer, "crown", 2).unwrap();
        tm.merge_declarations(&[decl2], &kp.public_key);
        assert_eq!(tm.declarations.len(), 1);
        assert_eq!(tm.declarations[0].trust_level, TrustLevel::Observer);

        // Lower version rejected
        let decl3 = crown::create_trust_declaration(&kp, "peer-1", TrustLevel::Untrusted, "crown", 1).unwrap();
        tm.merge_declarations(&[decl3], &kp.public_key);
        assert_eq!(tm.declarations[0].trust_level, TrustLevel::Observer);
    }
}
