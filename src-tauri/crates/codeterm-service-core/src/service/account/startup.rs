mod identity_writer;
pub(crate) use identity_writer::IdentityWriter;

use codeterm_account::{account_target::build_target, Identity, TargetMismatch};
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
    /// The account target that issued the saved credential; absent in records written before it.
    #[serde(default)]
    target: Option<String>,
}

#[derive(Clone)]
pub struct StartupAccount {
    backend: String,
    pub identity: Identity,
    grant: Option<VerifiedGrant>,
    valid: Arc<AtomicBool>,
    observed: Arc<AtomicU64>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    saved_target: Option<String>,
    mismatch: Option<TargetMismatch>,
}

pub fn remember(root: &Path, backend: &str, identity: Option<Identity>) -> Result<(), String> {
    remember_for_target(root, backend, identity, build_target())
}

fn remember_for_target(
    root: &Path,
    backend: &str,
    identity: Option<Identity>,
    target: &str,
) -> Result<(), String> {
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
            target: Some(target.into()),
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
    load_for_target(root, build_target())
}

fn load_for_target(root: &Path, build: &str) -> Option<StartupAccount> {
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
    let mismatch = TargetMismatch::detect(record.target.as_deref(), build);
    if let Some(mismatch) = &mismatch {
        log::warn!("Account target mismatch: {}", mismatch.reason);
    }
    Some(StartupAccount {
        backend: record.backend,
        identity: record.identity,
        grant,
        valid: Arc::new(AtomicBool::new(true)),
        observed: Arc::new(AtomicU64::new(observed)),
        clock: Arc::new(now),
        saved_target: record.target,
        mismatch,
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
        monotonic_now(&self.observed, (self.clock)())
    }
    /// Whether the saved sign-in may still be projected as signed in.
    pub fn is_valid(&self) -> bool {
        self.mismatch.is_none() && self.valid.load(Ordering::Acquire)
    }

    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }

    pub fn status(&self) -> codeterm_account::Status {
        codeterm_account::Status {
            provider: self.backend.clone(),
            identity: self.mismatch.is_none().then(|| self.identity.clone()),
            pending: false,
            session_only: false,
            server_stop_unconfirmed: false,
            session_revoked: false,
            last_sign_in_failure: None,
            target_mismatch: self.mismatch.clone(),
            purchases_enabled: codeterm_account::remote_config::purchases_enabled(),
        }
    }

    /// The status startup may show before the account restores: signed in, or the target mismatch.
    pub fn projected_status(&self) -> Option<codeterm_account::Status> {
        self.valid.load(Ordering::Acquire).then(|| self.status())
    }

    pub fn saved_target(&self) -> Option<&str> {
        self.saved_target.as_deref()
    }

    pub fn gate(&self) -> LimitGate {
        let Some(grant) = &self.grant else {
            return crate::service::billing::pending_limit_gate();
        };
        let verdict = grant.verdict(self.now());
        if !verdict.state.retains_grant() {
            return crate::service::billing::pending_limit_gate();
        }
        let access = verdict.effective();
        let deadline = (access.authority == codeterm_entitlements::Authority::Free).then(|| {
            let payload = grant.payload();
            payload
                .period_end
                .unwrap_or(payload.expires_at)
                .min(payload.expires_at)
        });
        let observed = self.observed.clone();
        let clock = self.clock.clone();
        LimitGate::new(access, codeterm_entitlements::enforcing()).with_deadline(
            deadline,
            Arc::new(move || monotonic_now(&observed, clock())),
        )
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
                            Plan::Paid("unavailable".into())
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

fn monotonic_now(observed: &AtomicU64, current: u64) -> u64 {
    observed.fetch_max(current, Ordering::SeqCst).max(current)
}

#[cfg(test)]
mod tests;
