use codeterm_account::{Account, Identity, TargetMismatch};
use codeterm_billing::{Access, Entitlements, Plan, Snapshot};
use codeterm_entitlements::{LimitGate, VerifiedGrant};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);
pub struct AccountDataRoot(pub PathBuf);
impl super::ServiceContext {
    pub fn account_data_root(&self) -> Result<PathBuf, String> {
        credential_root(self.try_ext::<AccountDataRoot>())
    }
}
pub fn credential_root(explicit: Option<&AccountDataRoot>) -> Result<PathBuf, String> {
    if let Some(root) = explicit {
        return Ok(root.0.clone());
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        Err("Account test fixture requires an isolated credential root".into())
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        Ok(crate::paths::data_dir_path())
    }
}
#[derive(Clone)]
pub struct StartupAccount {
    pub backend: String,
    pub identity: Identity,
    pub grant: Option<VerifiedGrant>,
    pub valid: Arc<AtomicBool>,
    pub observed: Arc<AtomicU64>,
    pub clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub saved_target: Option<String>,
    pub mismatch: Option<TargetMismatch>,
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
            verification: None,
            grant: verdict,
        })
    }

    pub fn plan(&self) -> Option<serde_json::Value> {
        let mut value = serde_json::to_value(self.snapshot()?).ok()?;
        value["limits"] = serde_json::to_value(self.gate().summary()).ok()?;
        Some(value)
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn monotonic_now(observed: &AtomicU64, current: u64) -> u64 {
    observed.fetch_max(current, Ordering::SeqCst).max(current)
}
