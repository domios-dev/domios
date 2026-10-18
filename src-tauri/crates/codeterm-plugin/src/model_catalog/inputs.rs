use super::*;

fn generations() -> &'static RwLock<HashMap<String, u64>> {
    static GENERATIONS: OnceLock<RwLock<HashMap<String, u64>>> = OnceLock::new();
    GENERATIONS.get_or_init(RwLock::default)
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
    serde_json::json!({
        "binary": provider.cli_binary().and_then(fingerprint_bin),
        "files": files,
        "generation": generation,
    })
    .to_string()
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
