use super::{
    Clock, Engine, FakeRegistry, InstallationProof, RegistryRefusal, RegistryResult, Session,
    Signature, EPOCH, INSTALLATION, PROJECT, STANDARD, URL_SAFE_NO_PAD,
};
use serde_json::json;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct ProofChallenges {
    sequence: u64,
    bodies: HashMap<String, (u64, serde_json::Value)>,
}

impl ProofChallenges {
    pub fn issue(&mut self, body: serde_json::Value, now: u64) -> String {
        self.sequence += 1;
        let nonce = format!("{now}.{:043}.{}", self.sequence, "b".repeat(43));
        self.bodies.insert(nonce.clone(), (now, body));
        nonce
    }

    pub fn expired(&self, nonce: &str, now: u64) -> bool {
        self.bodies
            .get(nonce)
            .is_some_and(|(issued, _)| now >= issued.saturating_add(120))
    }

    pub fn accepts(&self, body: &serde_json::Value, nonce: &str) -> bool {
        self.bodies.get(nonce).map(|(_, body)| body) == Some(body)
    }
}

impl FakeRegistry {
    pub(super) fn verify<T: serde::Serialize>(
        &self,
        session: &Session,
        endpoint: &str,
        body: &T,
        proof: &InstallationProof,
        spki: Option<&str>,
    ) -> RegistryResult<()> {
        let body = serde_json::to_value(body).unwrap();
        if endpoint == "meshAdmission"
            && body["operation"] == "recover"
            && self
                .challenged
                .lock()
                .unwrap()
                .expired(&proof.nonce, self.clock.now())
        {
            return Err(RegistryRefusal::ChallengeInvalid);
        }
        if !self.challenged.lock().unwrap().accepts(&body, &proof.nonce) {
            return Err(RegistryRefusal::Conflict);
        }
        let uid = &session.identity.uid;
        if spki.is_none()
            && self
                .accounts
                .lock()
                .unwrap()
                .get(uid)
                .is_some_and(|account| account.key_generation != proof.generation)
        {
            return Err(RegistryRefusal::GenerationMismatch);
        }
        let spki = match spki {
            Some(spki) => spki.to_owned(),
            None => self
                .accounts
                .lock()
                .unwrap()
                .get(uid)
                .and_then(|account| account.key.clone())
                .ok_or(RegistryRefusal::Forbidden)?,
        };
        let digest = crate::account_mesh::key::jcs_sha256_hex(&body).unwrap();
        let signed = json!({
            "domain": "domios.online-installation-proof.v1",
            "project": PROJECT, "mode": "test", "uid": uid, "installation_id": INSTALLATION,
            "policy_epoch": EPOCH, "method": "POST", "endpoint": endpoint, "body_sha256": digest,
            "schema_version": proof.schema_version, "key_id": proof.key_id,
            "generation": proof.generation, "timestamp": proof.timestamp, "nonce": proof.nonce,
        });
        let bytes = codeterm_entitlements::jcs::encode(&signed).unwrap();
        let der = STANDARD.decode(&spki).unwrap();
        let key = codeterm_entitlements::policy::parse_spki(&spki).unwrap();
        let signature: [u8; 64] = URL_SAFE_NO_PAD
            .decode(&proof.signature)
            .unwrap()
            .try_into()
            .unwrap();
        let valid = key
            .verify_strict(bytes.as_bytes(), &Signature::from_bytes(&signature))
            .is_ok()
            && proof.key_id == crate::account_mesh::key::sha256_hex(&der);
        valid.then_some(()).ok_or(RegistryRefusal::Forbidden)
    }
}
