use super::*;
use sha2::{Digest, Sha256};

fn generations() -> &'static crate::provider_account::cache::ScopedCache<HashMap<String, u64>> {
    static CACHE: crate::provider_account::cache::ScopedCache<HashMap<String, u64>> =
        crate::provider_account::cache::ScopedCache::new();
    &CACHE
}

pub fn invalidate_inputs(provider: &str) {
    advance_version(&mut generations().write(), provider);
    if let Some(snapshot) = registry().write().get_mut(provider) {
        snapshot.fetched_at = None;
    }
    invalidate(provider);
}

pub fn discovery_stamp(provider: &dyn AgentProvider) -> String {
    let generation = generations()
        .read()
        .get(provider.id())
        .copied()
        .unwrap_or(0);
    stamp(provider, generation)
}

fn stamp(provider: &dyn AgentProvider, generation: u64) -> String {
    let paths = provider
        .model_catalogue_capability()
        .fingerprint_files(provider.model_credential_files());
    let files: Vec<_> = paths
        .iter()
        .map(|path| {
            let metadata = std::fs::metadata(path).ok();
            let modified = metadata
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|time| time.as_nanos().to_string());
            (path, metadata.map(|m| m.len()), modified)
        })
        .collect();
    let inputs = serde_json::json!({
        "binary": provider.cli_binary().and_then(fingerprint_bin),
        "files": files,
        "generation": generation,
        "account": crate::provider_account::cache_key(provider.id()),
        "seed": seed_fingerprint(provider.id()),
    })
    .to_string();
    format!("{:x}", Sha256::digest(inputs.as_bytes()))
}

pub(super) fn publish_current<R>(
    provider: &dyn AgentProvider,
    expected: &str,
    publish: impl FnOnce() -> R,
) -> Option<R> {
    let generations = generations().read();
    let generation = generations.get(provider.id()).copied().unwrap_or(0);
    (stamp(provider, generation) == expected).then(publish)
}
