use super::manifest::{Manifest, VerifiedManifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Activation {
    pub active: Option<Manifest>,
    pub previous: Option<Manifest>,
    pub pending: Option<Manifest>,
    pub generation: u64,
    pub loading: Option<Loading>,
    pub quarantined: BTreeSet<String>,
    pub active_claims: BTreeMap<String, HealthClaim>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Loading {
    pub release_id: String,
    pub generation: u64,
    pub failure_deadline_ms: u64,
    pub clients: BTreeMap<String, HealthClaim>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthClaim {
    pub token: String,
    pub rendered: bool,
}

impl Activation {
    pub fn stage(&mut self, verified: &VerifiedManifest) -> Result<bool, String> {
        let manifest = verified.manifest();
        if self.loading.is_some() || self.quarantined.contains(&manifest.release_id) {
            return Err("Release loading or quarantined".into());
        }
        if self.active.as_ref() == Some(manifest) || self.pending.as_ref() == Some(manifest) {
            return Ok(false);
        }
        self.pending = Some(manifest.clone());
        Ok(true)
    }

    pub fn begin(
        &mut self,
        release_id: &str,
        clients: BTreeMap<String, String>,
        failure_deadline_ms: u64,
        api_version: u32,
    ) -> Result<u64, String> {
        if clients.is_empty() || clients.values().any(String::is_empty) {
            return Err("Activation already loading or missing client claims".into());
        }
        if let Some(loading) = self.loading.as_mut() {
            if loading.release_id != release_id {
                return Err("Another release is loading".into());
            }
            if clients.keys().any(|id| loading.clients.contains_key(id)) {
                return Err("Client already enrolled".into());
            }
            for (id, token) in clients {
                loading.clients.insert(
                    id,
                    HealthClaim {
                        token,
                        rendered: false,
                    },
                );
            }
            return Ok(loading.generation);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|m| m.release_id == release_id && m.compatible(api_version))
        {
            for (id, token) in clients {
                self.active_claims.insert(
                    id,
                    HealthClaim {
                        token,
                        rendered: false,
                    },
                );
            }
            return Ok(self.generation);
        }
        let pending = self.pending.as_ref().ok_or("No pending release")?;
        if pending.release_id != release_id
            || !pending.compatible(api_version)
            || self.quarantined.contains(release_id)
        {
            return Err("Invalid pending release".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Generation exhausted")?;
        self.loading = Some(Loading {
            release_id: release_id.into(),
            generation: self.generation,
            failure_deadline_ms,
            clients: clients
                .into_iter()
                .map(|(id, token)| {
                    (
                        id,
                        HealthClaim {
                            token,
                            rendered: false,
                        },
                    )
                })
                .collect(),
        });
        Ok(self.generation)
    }

    pub fn healthy(
        &mut self,
        release: &str,
        generation: u64,
        client: &str,
        token: &str,
    ) -> Result<bool, String> {
        if self.loading.is_none()
            && self.generation == generation
            && self
                .active
                .as_ref()
                .is_some_and(|m| m.release_id == release)
        {
            let claim = self.active_claims.get_mut(client).ok_or("Foreign client")?;
            if !crate::service::tokens::constant_time_eq(claim.token.as_bytes(), token.as_bytes()) {
                return Err("Foreign health token".into());
            }
            claim.rendered = true;
            return Ok(true);
        }
        let loading = self.loading.as_mut().ok_or("No loading generation")?;
        if loading.release_id != release || loading.generation != generation {
            return Err("Stale release generation".into());
        }
        let claim = loading.clients.get_mut(client).ok_or("Foreign client")?;
        if !crate::service::tokens::constant_time_eq(claim.token.as_bytes(), token.as_bytes()) {
            return Err("Foreign health token".into());
        }
        claim.rendered = true;
        if !loading.clients.values().all(|claim| claim.rendered) {
            return Ok(false);
        }
        self.active_claims = loading.clients.clone();
        self.previous = self.active.take();
        self.active = self.pending.take();
        self.loading = None;
        Ok(true)
    }

    pub fn expire(&mut self, now_ms: u64) -> bool {
        if self
            .loading
            .as_ref()
            .is_some_and(|loading| now_ms >= loading.failure_deadline_ms)
        {
            self.rollback_loading();
            true
        } else {
            false
        }
    }

    pub fn rollback_loading(&mut self) {
        if let Some(loading) = self.loading.take() {
            self.quarantined.insert(loading.release_id);
            self.pending = None;
        }
    }

    pub fn rollback_active(&mut self, release: &str, generation: u64) -> Result<(), String> {
        if self.generation != generation
            || self.active.as_ref().map(|m| m.release_id.as_str()) != Some(release)
        {
            return Err("Stale rollback generation".into());
        }
        self.quarantined.insert(release.into());
        self.active_claims.clear();
        self.active = self
            .previous
            .take()
            .filter(|m| !self.quarantined.contains(&m.release_id));
        self.pending = None;
        self.loading = None;
        Ok(())
    }

    pub fn recover(&mut self, api_version: u32) {
        self.rollback_loading();
        let eligible =
            |m: &Manifest| m.compatible(api_version) && !self.quarantined.contains(&m.release_id);
        if self.active.as_ref().is_some_and(|m| !eligible(m)) {
            self.active_claims.clear();
            self.active = self.previous.take().filter(eligible);
        }
        if self.pending.as_ref().is_some_and(|m| !eligible(m)) {
            self.pending = None;
        }
    }

    // The app's own frontend wins over any stored release that is not newer than it.
    pub fn retire_not_newer(&mut self, floor: &semver::Version) {
        let newer = |m: &Manifest| super::manifest::newer_than(&m.version, floor);
        self.pending = self.pending.take().filter(newer);
        self.previous = self.previous.take().filter(newer);
        if self.active.as_ref().is_some_and(|m| !newer(m)) {
            self.active_claims.clear();
            self.active = self.previous.take();
        }
    }

    pub fn public_status(&self) -> serde_json::Value {
        serde_json::json!({
            "active": self.active, "previous": self.previous, "pending": self.pending,
            "generation": self.generation,
            "loading": self.loading.as_ref().map(|l| serde_json::json!({"releaseId": l.release_id, "generation": l.generation})),
            "quarantined": self.quarantined,
        })
    }
}
