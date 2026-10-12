pub mod activation;
pub mod manifest;
pub mod store;
pub mod update;

pub const OBSERVER_SCRIPT: &str = include_str!("observer.js");

use activation::Activation;
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use store::Store;

pub type StatusEmitter = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct FrontendBundles {
    root: PathBuf,
    inner: Mutex<(Store, Activation)>,
    check_job: update::CheckJob,
    pub api_version: u32,
    bundled_version: semver::Version,
    public_key: Option<String>,
    discovery_url: String,
    allow_loopback: bool,
    emit: StatusEmitter,
}

impl FrontendBundles {
    pub fn open(
        root: &Path,
        api_version: u32,
        bundled_version: &str,
        public_key: Option<String>,
        discovery_url: String,
        allow_loopback: bool,
        emit: StatusEmitter,
    ) -> Result<Self, String> {
        if allow_loopback && !cfg!(debug_assertions) {
            return Err("DEV loopback forbidden in release".into());
        }
        let bundled_version = semver::Version::parse(bundled_version).map_err(|e| e.to_string())?;
        let mut store = Store::open(root)?;
        let mut state = store.load()?;
        state.recover(api_version);
        state.retire_not_newer(&bundled_version);
        for manifest in [
            state.active.clone(),
            state.previous.clone(),
            state.pending.clone(),
        ]
        .into_iter()
        .flatten()
        {
            let valid = public_key.as_ref().is_some_and(|key| {
                store
                    .verify_release(&manifest, key, api_version, allow_loopback)
                    .is_ok()
            });
            if !valid {
                state.quarantined.insert(manifest.release_id.clone());
            }
        }
        state.recover(api_version);
        if state.previous.as_ref().is_some_and(|m| {
            state.quarantined.contains(&m.release_id) || !m.compatible(api_version)
        }) {
            state.previous = None;
        }
        store.save(&state)?;
        Ok(Self {
            root: root.into(),
            inner: Mutex::new((store, state)),
            check_job: update::CheckJob::default(),
            api_version,
            bundled_version,
            public_key,
            discovery_url,
            allow_loopback,
            emit,
        })
    }

    pub fn status(&self) -> Value {
        let inner = self.inner.lock();
        let mut status = inner.1.public_status();
        status["apiVersion"] = self.api_version.into();
        status["updatesEnabled"] = self.public_key.is_some().into();
        status
    }

    pub fn check(&self) -> Result<Value, String> {
        let _lease = self.check_job.acquire()?;
        let key = self
            .public_key
            .as_deref()
            .ok_or("Frontend signing identity not provisioned")?;
        let store = Store::open(&self.root)?;
        let floor = self.version_floor();
        let verified = update::download_and_stage(
            &store,
            &self.discovery_url,
            &floor,
            key,
            self.api_version,
            self.allow_loopback,
        )?;
        let mut newly_pending = false;
        self.change(|state| {
            newly_pending = state.stage(&verified)?;
            Ok(())
        })?;
        let manifest = verified.manifest();
        let event = serde_json::json!({"releaseId": manifest.release_id, "version": manifest.version, "minApiVersion": manifest.min_api_version, "maxApiVersion": manifest.max_api_version});
        if newly_pending {
            (self.emit)("frontend-update-ready", event);
        }
        Ok(self.status())
    }

    fn version_floor(&self) -> semver::Version {
        let active = self
            .inner
            .lock()
            .1
            .active
            .as_ref()
            .and_then(|m| semver::Version::parse(&m.version).ok());
        active
            .filter(|v| v > &self.bundled_version)
            .unwrap_or_else(|| self.bundled_version.clone())
    }

    pub fn activate(
        &self,
        release: &str,
        clients: Vec<String>,
        deadline_ms: u64,
    ) -> Result<Value, String> {
        if clients.is_empty()
            || clients.len() > 128
            || clients.iter().any(|id| !manifest::safe_component(id))
        {
            return Err("Invalid client set".into());
        }
        let claims: BTreeMap<String, String> = clients
            .into_iter()
            .map(|id| (id, uuid::Uuid::new_v4().to_string()))
            .collect();
        let mut generation = 0;
        self.change(|state| {
            generation = state.begin(release, claims.clone(), deadline_ms, self.api_version)?;
            Ok(())
        })?;
        Ok(
            serde_json::json!({"releaseId": release, "generation": generation, "claims": claims, "index": format!("/frontend/{release}/index.html")}),
        )
    }

    pub fn healthy(
        &self,
        release: &str,
        generation: u64,
        client: &str,
        token: &str,
    ) -> Result<Value, String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64;
        self.failure_deadline(now_ms)?;
        self.change(|state| {
            state.healthy(release, generation, client, token)?;
            Ok(())
        })?;
        Ok(self.status())
    }

    pub fn fail(&self, release: &str, generation: u64) -> Result<Value, String> {
        self.change(|state| {
            if state.loading.as_ref().is_some_and(|loading| {
                loading.release_id == release && loading.generation == generation
            }) {
                state.rollback_loading();
                Ok(())
            } else {
                state.rollback_active(release, generation)
            }
        })?;
        let status = self.status();
        (self.emit)("frontend-status", status.clone());
        Ok(status)
    }

    pub fn failure_deadline(&self, now_ms: u64) -> Result<bool, String> {
        let mut expired = false;
        self.change(|state| {
            expired = state.expire(now_ms);
            Ok(())
        })?;
        if expired {
            (self.emit)("frontend-status", self.status());
        }
        Ok(expired)
    }

    pub fn active_index(&self) -> Option<String> {
        self.inner
            .lock()
            .1
            .active
            .as_ref()
            .map(|m| format!("/frontend/{}/index.html", m.release_id))
    }

    pub fn asset(&self, release: &str, path: &str) -> Result<Vec<u8>, String> {
        self.inner.lock().0.asset(release, path)
    }

    pub fn checkpoint(&self, client: &str) -> Result<Value, String> {
        if !manifest::safe_component(client) {
            return Err("Invalid client ID".into());
        }
        let checkpoint = self.inner.lock().0.checkpoint(client)?;
        Ok(checkpoint
            .map(
                |(revision, payload)| serde_json::json!({"revision": revision, "payload": payload}),
            )
            .unwrap_or(Value::Null))
    }

    pub fn put_checkpoint(
        &self,
        client: &str,
        revision: u64,
        payload: Value,
    ) -> Result<Value, String> {
        if !manifest::safe_component(client) {
            return Err("Invalid client ID".into());
        }
        self.inner
            .lock()
            .0
            .put_checkpoint(client, revision, &payload)?;
        Ok(serde_json::json!({"revision": revision, "payload": payload}))
    }

    fn change(
        &self,
        action: impl FnOnce(&mut Activation) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock();
        let mut candidate = inner.1.clone();
        action(&mut candidate)?;
        inner.0.save(&candidate)?;
        inner.1 = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
