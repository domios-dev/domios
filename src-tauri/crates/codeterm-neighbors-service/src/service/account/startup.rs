pub use codeterm_service_core::service::neighbor_account::{monotonic_now, now, StartupAccount};
pub mod identity_writer;
pub use identity_writer::IdentityWriter;

use codeterm_account::{account_target::build_target, Identity};
use codeterm_billing::{FileGrantStore, GrantStore};
use codeterm_util::secret_file::SecretFile;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
};

const IDENTITY_KEY: &str = "startup-identity";

#[derive(Serialize, Deserialize)]
pub struct LocalIdentity {
    backend: String,
    identity: Identity,
    /// The account target that issued the saved credential; absent in records written before it.
    #[serde(default)]
    target: Option<String>,
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

#[cfg(test)]
pub mod tests;
