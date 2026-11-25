use super::{
    credential_fingerprints, fingerprint_bin, models_dir, AgentProvider, BinaryFingerprint,
    ModelInfo,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Scope {
    account: String,
    credentials: Vec<BinaryFingerprint>,
    cli_binary: String,
    binary: BinaryFingerprint,
    cli_version: String,
}

impl Scope {
    fn is_current(&self, account: &str) -> bool {
        self.account == account
            && !self.credentials.is_empty()
            && !super::credentials_moved(&self.credentials)
            && fingerprint_bin(&self.cli_binary).as_ref() == Some(&self.binary)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Refusals {
    scope: Option<Scope>,
    models: Vec<String>,
}

impl Refusals {
    fn remember(&mut self, scope: Scope, model: &str) -> bool {
        if self.scope.as_ref() != Some(&scope) {
            self.scope = Some(scope);
            self.models.clear();
        }
        if self.models.iter().any(|id| id == model) {
            return false;
        }
        if self.models.len() == 128 {
            self.models.remove(0);
        }
        self.models.push(model.into());
        true
    }
}

static REFUSALS: LazyLock<Mutex<HashMap<PathBuf, Refusals>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn read(path: &Path) -> Refusals {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn path(account: &str) -> PathBuf {
    models_dir().join(format!("{account}.refusals.json"))
}

pub fn remember(
    provider: &dyn AgentProvider,
    launched_binary: &BinaryFingerprint,
    cli_version: &str,
    model: &str,
) -> std::io::Result<()> {
    let Some(cli_binary) = provider.cli_binary() else {
        return Ok(());
    };
    let Some(binary) = fingerprint_bin(cli_binary) else {
        return Ok(());
    };
    let credentials = credential_fingerprints(provider);
    if &binary != launched_binary
        || credentials.is_empty()
        || cli_version.is_empty()
        || model.is_empty()
        || model.len() > 256
    {
        return Ok(());
    }
    let account = crate::provider_account::cache_key(provider.id());
    let scope = Scope {
        account: account.clone(),
        credentials,
        cli_binary: cli_binary.into(),
        binary,
        cli_version: cli_version.into(),
    };
    let path = path(&account);
    let changed = {
        let mut cache = REFUSALS.lock();
        let refusals = cache.entry(path.clone()).or_insert_with(|| read(&path));
        let mut updated = refusals.clone();
        let changed = updated.remember(scope, model);
        if changed {
            codeterm_util::private_file::write_atomic(&path, &serde_json::to_vec(&updated)?)?;
            *refusals = updated;
        }
        changed
    };
    if changed {
        super::invalidate(provider.id());
    }
    Ok(())
}

pub(super) fn filter(provider: &str, models: &mut Vec<ModelInfo>) {
    let unavailable = unavailable_models(provider);
    models.retain(|model| !unavailable.contains(&model.id));
}

pub fn unavailable_models(provider: &str) -> Vec<String> {
    unavailable_models_for_version(provider, None)
}

pub fn unavailable_models_for_version(provider: &str, cli_version: Option<&str>) -> Vec<String> {
    let account = crate::provider_account::cache_key(provider);
    let path = path(&account);
    let mut cache = REFUSALS.lock();
    let refusals = cache.entry(path.clone()).or_insert_with(|| read(&path));
    if refusals.scope.as_ref().is_some_and(|scope| {
        scope.is_current(&account) && cli_version.is_none_or(|version| scope.cli_version == version)
    }) {
        return refusals.models.clone();
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
