use super::{
    resolve_default, resolve_default_reasoning_effort, resolve_models, resolve_reasoning_efforts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::OnceLock};
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct ModelCatalogueChanged {
    pub provider: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context_key: Option<String>,
}

#[derive(Clone, Default)]
pub(super) struct PublishedCatalogue {
    version: u64,
    digest: Option<[u8; 32]>,
    account_id: Option<String>,
    context_key: Option<String>,
}

pub(super) fn versions(
) -> &'static crate::provider_account::cache::ScopedCache<HashMap<String, PublishedCatalogue>> {
    static CACHE: crate::provider_account::cache::ScopedCache<HashMap<String, PublishedCatalogue>> =
        crate::provider_account::cache::ScopedCache::new();
    &CACHE
}

fn change_sender() -> &'static broadcast::Sender<ModelCatalogueChanged> {
    static CHANGES: OnceLock<broadcast::Sender<ModelCatalogueChanged>> = OnceLock::new();
    CHANGES.get_or_init(|| broadcast::channel(256).0)
}

pub fn subscribe_changes() -> broadcast::Receiver<ModelCatalogueChanged> {
    change_sender().subscribe()
}

pub fn current_version(provider_id: &str) -> u64 {
    versions()
        .read()
        .get(provider_id)
        .map(|published| published.version)
        .unwrap_or_default()
}

pub fn version_snapshot() -> Vec<ModelCatalogueChanged> {
    let mut changes = versions()
        .read()
        .iter()
        .map(|(provider, published)| ModelCatalogueChanged {
            provider: provider.clone(),
            version: published.version,
            account_id: published.account_id.clone(),
            context_key: published.context_key.clone(),
        })
        .collect::<Vec<_>>();
    changes.sort_by(|left, right| left.provider.cmp(&right.provider));
    changes
}

pub(super) fn advance_version(versions: &mut HashMap<String, u64>, provider_id: &str) -> u64 {
    let version = versions.entry(provider_id.to_string()).or_default();
    *version = version.saturating_add(1);
    *version
}

fn publish_digest(
    versions: &mut HashMap<String, PublishedCatalogue>,
    provider_id: &str,
    digest: [u8; 32],
    scope: &(Option<String>, Option<String>),
) -> Option<ModelCatalogueChanged> {
    let published = versions.entry(provider_id.to_string()).or_default();
    if published.digest == Some(digest) {
        return None;
    }
    published.version = published.version.saturating_add(1);
    published.digest = Some(digest);
    published.account_id = scope.0.clone();
    if published.context_key.is_none() {
        published.context_key = scope
            .1
            .as_ref()
            .map(|key| format!("{key}:{}", uuid::Uuid::new_v4()));
    }
    Some(ModelCatalogueChanged {
        provider: provider_id.to_string(),
        version: published.version,
        account_id: published.account_id.clone(),
        context_key: published.context_key.clone(),
    })
}

fn catalogue_digest(provider_id: &str) -> [u8; 32] {
    let catalogue = serde_json::json!({
        "available_models": resolve_models(provider_id),
        "default_model": resolve_default(provider_id),
        "reasoning_efforts": resolve_reasoning_efforts(provider_id),
        "default_reasoning_effort": resolve_default_reasoning_effort(provider_id),
    });
    Sha256::digest(serde_json::to_vec(&catalogue).expect("model catalogue serializes")).into()
}

pub fn invalidate(provider_id: &str) -> ModelCatalogueChanged {
    let digest = catalogue_digest(provider_id);
    let scope = scope_fields(provider_id);
    let change = {
        let mut versions = versions().write();
        publish_digest(&mut versions, provider_id, digest, &scope)
    };
    if let Some(change) = change {
        log::debug!(
            "[model_catalog] published changed catalogue provider={} version={}",
            change.provider,
            change.version
        );
        let _ = change_sender().send(change.clone());
        change
    } else {
        versions()
            .read()
            .get(provider_id)
            .map(|published| ModelCatalogueChanged {
                provider: provider_id.to_string(),
                version: published.version,
                account_id: published.account_id.clone(),
                context_key: published.context_key.clone(),
            })
            .unwrap_or(ModelCatalogueChanged {
                provider: provider_id.to_string(),
                version: 0,
                account_id: scope.0,
                context_key: scope.1,
            })
    }
}

fn scope_fields(provider: &str) -> (Option<String>, Option<String>) {
    match crate::provider_account::current() {
        Some(context) => (
            (context.provider() == provider).then(|| context.account().to_owned()),
            Some(format!(
                "{}:{}",
                context.cache_key(),
                super::seed_fingerprint(context.provider())
            )),
        ),
        None => (None, None),
    }
}

pub fn all_version_snapshots() -> Vec<ModelCatalogueChanged> {
    let mut snapshots = versions()
        .snapshots()
        .into_iter()
        .flat_map(|versions| {
            versions
                .into_iter()
                .map(|(provider, published)| ModelCatalogueChanged {
                    provider,
                    version: published.version,
                    account_id: published.account_id,
                    context_key: published.context_key,
                })
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|left, right| {
        (&left.provider, &left.account_id, &left.context_key).cmp(&(
            &right.provider,
            &right.account_id,
            &right.context_key,
        ))
    });
    snapshots
}
