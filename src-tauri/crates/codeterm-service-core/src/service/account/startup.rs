use codeterm_account::Identity;
use codeterm_billing::{Access, Entitlements, FileGrantStore, GrantStore, Plan, Snapshot};
use codeterm_entitlements::{LimitGate, VerifiedGrant};
use codeterm_util::secret_file::SecretFile;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const IDENTITY_KEY: &str = "startup-identity";

#[derive(Serialize, Deserialize)]
struct LocalIdentity {
    backend: String,
    identity: Identity,
}

#[derive(Clone)]
pub struct StartupAccount {
    backend: String,
    pub identity: Identity,
    grant: Option<VerifiedGrant>,
    valid: Arc<AtomicBool>,
    observed: Arc<AtomicU64>,
}

pub fn remember(root: &Path, backend: &str, identity: Option<Identity>) -> Result<(), String> {
    let file = SecretFile::new_at(root, "domios.account");
    if let Some(identity) = identity {
        if file
            .get(&super::credentials::credential_name(backend))
            .is_err()
        {
            return file.delete(IDENTITY_KEY).map_err(|error| error.to_string());
        }
        let record = LocalIdentity {
            backend: backend.into(),
            identity,
        };
        file.set(
            IDENTITY_KEY,
            &serde_json::to_string(&record).map_err(|error| error.to_string())?,
        )
    } else {
        file.delete(IDENTITY_KEY)
    }
    .map_err(|error| error.to_string())
}

pub fn load(root: &Path) -> Option<StartupAccount> {
    let file = SecretFile::new_at(root, "domios.account");
    let record: LocalIdentity = serde_json::from_str(&file.get(IDENTITY_KEY).ok()?).ok()?;
    file.get(&super::credentials::credential_name(&record.backend))
        .ok()?;
    let installation = codeterm_account::Account::installation_id(
        &codeterm_account::FileInstallationStore::new(root.into()),
    )
    .ok()?;
    let store = FileGrantStore::new(root.into());
    let grant =
        codeterm_billing::startup_grant(&store, &record.identity.uid, installation.as_str(), now());
    let observed = store
        .load_floors()
        .ok()
        .flatten()
        .map_or_else(now, |floors| floors.observed_at.max(now()));
    Some(StartupAccount {
        backend: record.backend,
        identity: record.identity,
        grant,
        valid: Arc::new(AtomicBool::new(true)),
        observed: Arc::new(AtomicU64::new(observed)),
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl StartupAccount {
    fn now(&self) -> u64 {
        monotonic_now(&self.observed)
    }
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }

    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }

    pub fn status(&self) -> codeterm_account::Status {
        codeterm_account::Status {
            provider: self.backend.clone(),
            identity: Some(self.identity.clone()),
            pending: false,
            session_only: false,
            server_stop_unconfirmed: false,
            session_revoked: false,
            last_sign_in_failure: None,
        }
    }

    pub fn gate(&self) -> LimitGate {
        let Some(grant) = &self.grant else {
            return crate::service::billing::pending_limit_gate();
        };
        let payload = grant.payload();
        let deadline = payload
            .period_end
            .unwrap_or(payload.expires_at)
            .min(payload.expires_at);
        let observed = self.observed.clone();
        LimitGate::new(
            grant.verdict(self.now()).effective(),
            codeterm_entitlements::enforcing(),
        )
        .with_deadline(Some(deadline), Arc::new(move || monotonic_now(&observed)))
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        if !self.is_valid() {
            return None;
        }
        let verdict = self.grant.as_ref().map(|grant| grant.verdict(self.now()));
        Some(Snapshot {
            entitlements: Entitlements {
                plan: verdict.as_ref().map_or_else(
                    || Plan::Paid("unavailable".into()),
                    |verdict| {
                        if verdict.state.retains_grant() {
                            Plan::from(&verdict.label)
                        } else {
                            Plan::Free
                        }
                    },
                ),
                ..Default::default()
            },
            access: verdict
                .as_ref()
                .map(|verdict| Access::from_effective(&verdict.effective())),
            stale: true,
            checking: true,
            grant: verdict,
        })
    }

    pub fn plan(&self) -> Option<serde_json::Value> {
        let mut value = serde_json::to_value(self.snapshot()?).ok()?;
        value["limits"] = serde_json::to_value(self.gate().summary()).ok()?;
        Some(value)
    }
}

fn monotonic_now(observed: &AtomicU64) -> u64 {
    let current = now();
    observed.fetch_max(current, Ordering::SeqCst).max(current)
}

#[cfg(test)]
mod tests;
