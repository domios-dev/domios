use super::*;

/// Why a live probe published nothing. The previous snapshot stays active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshFailure {
    NoLiveList,
    OnlyNonChatModels,
    InputsChangedDuringDiscovery,
    TimedOut(Duration),
    Aborted,
}

impl std::fmt::Display for RefreshFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLiveList => write!(f, "the provider reported no live model list"),
            Self::OnlyNonChatModels => {
                write!(f, "the provider listed only non-chat models")
            }
            Self::InputsChangedDuringDiscovery => write!(
                f,
                "the provider binary or catalogue files changed during discovery"
            ),
            Self::TimedOut(limit) => {
                write!(f, "discovery timed out after {}s", limit.as_secs())
            }
            Self::Aborted => write!(f, "the discovery task stopped before answering"),
        }
    }
}

fn failures() -> &'static RwLock<HashMap<String, RefreshFailure>> {
    static FAILURES: OnceLock<RwLock<HashMap<String, RefreshFailure>>> = OnceLock::new();
    FAILURES.get_or_init(RwLock::default)
}

/// The reason the provider's most recent live probe published nothing; cleared
/// by the next probe that publishes.
pub fn last_refresh_failure(provider_id: &str) -> Option<RefreshFailure> {
    failures().read().get(provider_id).cloned()
}

fn record(provider_id: &str, outcome: &Result<Vec<ModelInfo>, RefreshFailure>) {
    match outcome {
        Ok(_) => {
            failures().write().remove(provider_id);
        }
        Err(failure) => {
            log::warn!("[model_catalog] {provider_id} live discovery did not publish: {failure}");
            failures()
                .write()
                .insert(provider_id.to_string(), failure.clone());
        }
    }
}

/// Refresh live models off the async runtime. On timeout / opt-out / failure — or an
/// empty post-sanitize result — the previous snapshot stays active (the list never goes empty).
pub async fn refresh_live(
    provider: Arc<dyn AgentProvider>,
    timeout_duration: Duration,
) -> Option<Vec<ModelInfo>> {
    let provider_id = provider.id().to_string();
    let default = provider.default_model();
    let publish_id = provider_id.clone();
    // blocking pool. Use an independent executor, not `Handle::block_on` (deadlocks nested in spawn_blocking).
    let task = tokio::task::spawn_blocking(move || {
        // A refresh must observe current filesystem and PATH state before live discovery.
        crate::js_runtime::forget_resolved_bins();
        let inputs = discovery_stamp(provider.as_ref());
        endpoints::refresh(provider.as_ref(), &inputs);
        let models = futures_executor::block_on(provider.discover_models())
            .ok_or(RefreshFailure::NoLiveList)?;
        let was_empty = models.is_empty();
        let models = sanitize_live(models);
        if !was_empty && models.is_empty() {
            return Err(RefreshFailure::OnlyNonChatModels);
        }
        let default = resolve_live_default(&models, default);
        let binary = provider.cli_binary().and_then(fingerprint_bin);
        let credentials = credential_fingerprints(provider.as_ref());
        inputs::publish_current(provider.as_ref(), &inputs, || {
            put_live_with_credentials(&publish_id, models.clone(), default, binary, credentials);
            models
        })
        .ok_or(RefreshFailure::InputsChangedDuringDiscovery)
    });
    let outcome = match tokio::time::timeout(timeout_duration, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(RefreshFailure::Aborted),
        Err(_) => Err(RefreshFailure::TimedOut(timeout_duration)),
    };
    record(&provider_id, &outcome);
    outcome.ok()
}
