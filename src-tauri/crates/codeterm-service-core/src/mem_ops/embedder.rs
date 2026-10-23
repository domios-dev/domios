use std::sync::Arc;
use std::time::Duration;

use codeterm_mem::embedder::catalog::{self, BuildContext, BuildError, EmbedderSelection};
use codeterm_mem::embedder::key::{self, ApiKey, KeyPresence, KeySources, KeyVault};
use codeterm_mem::embedder::{
    EmbedderIdentity, EmbedderStatus, ProviderDescriptor, ReembedState, SwitchPlan,
};
use serde::{Deserialize, Serialize};

use crate::config::ConfigState;
use crate::secrets::SecretStoreState;
use crate::service::ServiceState;

pub const CHANGED_EVENT: &str = "mem-embedder-changed";
const VAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The Domios secret store as the embedder's fallback key source. Each call
/// runs on its own thread so it is safe from async and blocking callers alike.
struct SecretStoreVault(SecretStoreState);

impl SecretStoreVault {
    fn run<T: Send>(
        &self,
        name: &str,
        call: impl FnOnce(
                SecretStoreState,
                String,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>
            + Send,
    ) -> Result<T, String> {
        let (store, name) = (self.0.clone(), name.to_string());
        std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    // The timer must be created inside the runtime, not on this bare thread.
                    crate::async_runtime::block_on(async move {
                        tokio::time::timeout(VAULT_TIMEOUT, call(store, name)).await
                    })
                })
                .join()
        })
        .map_err(|_| "secret store lookup panicked".to_string())?
        .map_err(|_| "secret store lookup timed out".to_string())
    }
}

impl KeyVault for SecretStoreVault {
    fn contains(&self, name: &str) -> Result<bool, String> {
        self.run(name, |store, name| {
            Box::pin(async move { store.read().await.has(&name).await })
        })?
        .map_err(|error| error.to_string())
    }

    fn read(&self, name: &str) -> Result<Option<ApiKey>, String> {
        let value = self.run(name, |store, name| {
            Box::pin(async move { store.read().await.get(&name).await })
        })?;
        match value {
            Ok(secret) => Ok(ApiKey::new(secret.value.expose_secret())),
            Err(crate::secrets::SecretError::NotFound) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn build_context(svc: &ServiceState) -> BuildContext {
    BuildContext {
        keys: KeySources::new(
            crate::service::general_agent::secrets_dir(),
            Some(Arc::new(SecretStoreVault(svc.secrets.clone()))),
        ),
        endpoint: None,
    }
}

fn config(svc: &ServiceState) -> codeterm_types::config::EmbedderConfig {
    svc.try_ext::<ConfigState>()
        .map(|state| state.lock().memory.embedder.clone())
        .unwrap_or_default()
}

fn selection(config: &codeterm_types::config::EmbedderConfig) -> EmbedderSelection {
    EmbedderSelection {
        provider: config.provider.clone(),
        model: Some(config.model.clone()).filter(|model| !model.is_empty()),
        dimension: Some(config.dimension).filter(|dimension| *dimension > 0),
    }
}

fn stamped_selection(identity: &EmbedderIdentity) -> EmbedderSelection {
    EmbedderSelection {
        provider: identity.provider.clone(),
        model: Some(identity.model.clone()),
        dimension: Some(identity.dimension),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderListing {
    #[serde(flatten)]
    pub descriptor: ProviderDescriptor,
    pub active: bool,
    pub key: Option<KeyPresence>,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub message: String,
    pub hints: Vec<String>,
}

impl From<&BuildError> for Refusal {
    fn from(error: &BuildError) -> Self {
        Self {
            message: error.message(),
            hints: error.hints(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbedderListing {
    pub providers: Vec<ProviderListing>,
    pub remote_opt_in: bool,
    pub remote_warning: &'static str,
    pub status: EmbedderStatus,
}

pub fn list(svc: &ServiceState) -> EmbedderListing {
    let status = svc.mem.embedder_status();
    let opt_in = config(svc).remote_opt_in;
    let keys = build_context(svc).keys;
    let providers = catalog::catalog()
        .into_iter()
        .map(|descriptor| {
            let key = descriptor.remote.then(|| keys.presence(&descriptor.id));
            let refusal = key.as_ref().and_then(|presence| {
                key::check_remote(&descriptor.id, true, opt_in, presence)
                    .err()
                    .map(|refusal| Refusal::from(&BuildError::Refused(refusal)))
            });
            ProviderListing {
                active: descriptor.id == status.provider,
                descriptor,
                key,
                refusal,
            }
        })
        .collect();
    EmbedderListing {
        providers,
        remote_opt_in: opt_in,
        remote_warning: key::REMOTE_WARNING,
        status,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UseParams {
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dimension: Option<usize>,
}

impl UseParams {
    fn selection(&self) -> EmbedderSelection {
        EmbedderSelection {
            provider: self.provider.clone(),
            model: self.model.clone().filter(|model| !model.is_empty()),
            dimension: self.dimension.filter(|dimension| *dimension > 0),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SwitchPreview {
    pub already_active: bool,
    pub cancelled: Option<EmbedderIdentity>,
    pub target: EmbedderIdentity,
    pub remote: bool,
    pub warning: Option<&'static str>,
    pub plan: SwitchPlan,
    pub total: u64,
    pub refusal: Option<Refusal>,
}

/// What a switch would do, without doing it: the target, the item count and
/// any refusal the gate would raise.
pub fn preview(svc: &ServiceState, params: &UseParams) -> Result<SwitchPreview, Refusal> {
    let selection = params.selection();
    let (descriptor, model, dimension) =
        catalog::resolve(&selection).map_err(|error| Refusal::from(&error))?;
    let plan = svc.mem.reembed_plan().map_err(|message| Refusal {
        message,
        hints: Vec::new(),
    })?;
    let keys = build_context(svc).keys;
    let refusal = key::check_remote(
        &descriptor.id,
        descriptor.remote,
        config(svc).remote_opt_in,
        &keys.presence(&descriptor.id),
    )
    .err()
    .map(|refusal| Refusal::from(&BuildError::Refused(refusal)));
    Ok(SwitchPreview {
        cancelled: None,
        already_active: svc.mem.embedder().identity()
            == EmbedderIdentity {
                provider: descriptor.id.clone(),
                model: model.model.clone(),
                dimension,
            },
        target: EmbedderIdentity {
            provider: descriptor.id.clone(),
            model: model.model,
            dimension,
        },
        remote: descriptor.remote,
        warning: descriptor.remote.then_some(key::REMOTE_WARNING),
        total: plan.total(),
        plan,
        refusal,
    })
}

/// Activate a provider through the remote gate, switch and re-embed in the
/// background, then persist it as `memory.embedder` once the switch is accepted.
pub fn use_provider(svc: &ServiceState, params: &UseParams) -> Result<SwitchPreview, Refusal> {
    let selection = params.selection();
    let opt_in = config(svc).remote_opt_in;
    let provider = catalog::activate(&selection, &build_context(svc), opt_in)
        .map_err(|error| Refusal::from(&error))?;
    let target = provider.identity();
    let remote = provider.remote();
    let already_active = svc.mem.embedder().identity() == target;
    let running = svc.mem.embedder_status().reembed;
    let cancelled = (already_active && running.state == ReembedState::Running)
        .then_some(running.target)
        .flatten();
    let plan = svc
        .mem
        .switch_embedder(provider)
        .map_err(|message| Refusal {
            message,
            hints: Vec::new(),
        })?;
    persist(svc, &target)?;
    Ok(SwitchPreview {
        already_active,
        cancelled,
        target,
        remote,
        warning: remote.then_some(key::REMOTE_WARNING),
        total: plan.total(),
        plan,
        refusal: None,
    })
}

fn persist(svc: &ServiceState, target: &EmbedderIdentity) -> Result<(), Refusal> {
    let Some(state) = svc.try_ext::<ConfigState>() else {
        return Ok(());
    };
    let mut next = state.lock().clone();
    next.memory.embedder.provider = target.provider.clone();
    next.memory.embedder.model = target.model.clone();
    next.memory.embedder.dimension = target.dimension;
    crate::config::save_config_and_apply(&state, next).map_err(|message| Refusal {
        message,
        hints: Vec::new(),
    })
}

/// Daemon start: push embedder changes to the UI, then activate the
/// configured provider off the startup path. A configured provider that the
/// gate refuses falls back to the model already stamped on the index.
pub fn start(svc: &ServiceState) {
    let ui = svc.ui.clone();
    let mem = Arc::downgrade(&svc.mem);
    svc.mem.embedder().subscribe(Arc::new(move |_event| {
        if let Some(mem) = mem.upgrade() {
            let status = serde_json::to_value(mem.embedder_status()).unwrap_or_default();
            ui.emit(CHANGED_EVENT, status);
        }
    }));
    let svc = svc.clone();
    let spawned = std::thread::Builder::new()
        .name("mem-embedder-start".into())
        .spawn(move || activate_configured(&svc));
    if let Err(error) = spawned {
        log::warn!("[mem.embedder] startup activation not started: {error}");
    }
}

fn activate_configured(svc: &ServiceState) {
    let configured = config(svc);
    let ctx = build_context(svc);
    let provider = match catalog::activate(&selection(&configured), &ctx, configured.remote_opt_in)
    {
        Ok(provider) => provider,
        Err(error) => {
            log::warn!(
                "[mem.embedder] configured embedder not activated: {}",
                error.message()
            );
            let Some(stamp) = svc.mem.embedder_status().index else {
                return;
            };
            let remote = catalog::descriptor(&stamp.provider).is_some_and(|row| row.remote);
            if remote && !configured.remote_opt_in {
                return;
            }
            match catalog::build(&stamped_selection(&stamp), &ctx) {
                Ok(provider) => provider,
                Err(_) => return,
            }
        }
    };
    if let Some(stamp) = svc
        .mem
        .embedder_status()
        .index
        .filter(|stamp| *stamp != provider.identity())
    {
        match catalog::activate(&stamped_selection(&stamp), &ctx, configured.remote_opt_in) {
            Ok(published) => {
                svc.mem.adopt_published(published);
            }
            Err(error) => {
                log::warn!(
                    "[mem.embedder] published provider unavailable during resume: {}",
                    error.message()
                );
            }
        }
    }
    match svc.mem.reconcile_embedder(provider) {
        Ok(Some(plan)) => log::info!(
            "[mem.embedder] startup switch re-embeds {} items in the background",
            plan.total()
        ),
        Ok(None) => {}
        Err(error) => log::warn!("[mem.embedder] startup activation failed: {error}"),
    }
}
