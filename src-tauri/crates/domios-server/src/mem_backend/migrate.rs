use super::{postgres_url, MemBackendError, MemBackendOpener};
use codeterm_service_core::{
    mem::{
        backend::{
            migrate::{content_digests, transfer_verified, verify_counts},
            MemStore, StoreTable, TableCounts, WriteContext,
        },
        embedder::{
            catalog::{self, BuildContext, EmbedderSelection},
            key::KeySources,
            ActiveEmbedder,
        },
    },
    secrets, PathOwnership, ResolvedInstancePaths,
};
use codeterm_types::config::MemoryBackend;
use std::sync::Arc;

#[derive(Debug)]
pub enum MigrationError {
    DaemonRunning,
    SchemaBusy { side: MemoryBackend },
    Backend(MemBackendError),
    Failed(String),
}
impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaBusy { side } => {
                write!(f, "memory_migration_schema_busy: {side} schema is in use")
            }
            Self::DaemonRunning => {
                f.write_str("memory_migration_daemon_running: stop the daemon first")
            }
            Self::Backend(error) => error.fmt(f),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}
impl std::error::Error for MigrationError {}
impl From<String> for MigrationError {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}
impl From<MemBackendError> for MigrationError {
    fn from(value: MemBackendError) -> Self {
        Self::Backend(value)
    }
}

pub fn ownership_decision(error: std::io::Error) -> MigrationError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        MigrationError::DaemonRunning
    } else {
        MigrationError::Failed(format!("memory_migration_lock: {error}"))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub to: MemoryBackend,
    pub into_nonempty: bool,
    pub json: bool,
}

pub fn parse(args: &[String]) -> Result<Options, MigrationError> {
    let mut to = None;
    let mut into_nonempty = false;
    let mut json = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" if to.is_none() => {
                to = Some(match args.next().map(String::as_str) {
                    Some("sqlite") => MemoryBackend::Sqlite,
                    Some("postgres") => MemoryBackend::Postgres,
                    _ => {
                        return Err(
                            "memory_migration_arguments: --to requires sqlite or postgres"
                                .to_string()
                                .into(),
                        )
                    }
                })
            }
            "--into-nonempty" if !into_nonempty => into_nonempty = true,
            "--json" if !json => json = true,
            _ => {
                return Err("memory_migration_arguments: unknown or repeated option"
                    .to_string()
                    .into())
            }
        }
    }
    Ok(Options {
        to: to.ok_or("memory_migration_arguments: --to is required".to_string())?,
        into_nonempty,
        json,
    })
}

pub fn execute(options: &Options) -> Result<serde_json::Value, MigrationError> {
    let identity = ResolvedInstancePaths::current().map_err(|error| error.to_string())?;
    let registry = crate::ownership_registry().map_err(|error| error.to_string())?;
    let _owner =
        PathOwnership::try_acquire(identity.clone(), &registry).map_err(ownership_decision)?;
    let text = std::fs::read_to_string(identity.config())
        .map_err(|error| format!("memory_migration_config: {error}"))?;
    let config: codeterm_types::config::schema::AppConfig =
        toml::from_str(&text).map_err(|error| format!("memory_migration_config: {error}"))?;
    let secret_registry = Arc::new(secrets::SecretRegistry::new());
    let url = postgres_url(secrets::build_active_store(
        &config.integrations.secrets,
        &secret_registry,
    ))?;
    let opener = MemBackendOpener {
        data_dir: identity.data(),
        postgres_url: url.as_ref().map(|url| url.expose_secret()),
    };
    // Schema claims precede all opens: source, target, then bootstrap inside open.
    let source_claim = opener.migration_claim(options.to.other())?;
    let target_claim = opener.migration_claim(options.to)?;
    let source = opener.open_migration(options.to.other(), true, source_claim.as_ref())?;
    let target = opener.open_migration(options.to, false, target_claim.as_ref())?;
    let source_stamp = source.vector_stamp()?;
    let target_stamp = target.vector_stamp()?;
    let embedder = &config.memory.embedder;
    let selection = target_stamp
        .map(|stamp| EmbedderSelection {
            provider: stamp.provider,
            model: Some(stamp.model),
            dimension: Some(stamp.dimension),
        })
        .unwrap_or_else(|| EmbedderSelection {
            provider: embedder.provider.clone(),
            model: Some(embedder.model.clone()).filter(|model| !model.is_empty()),
            dimension: Some(embedder.dimension).filter(|dimension| *dimension > 0),
        });
    let (_, model, dimension) = catalog::resolve(&selection).map_err(|error| error.message())?;
    let desired = codeterm_service_core::mem::embedder::EmbedderIdentity {
        provider: selection.provider.clone(),
        model: model.model,
        dimension,
    };
    let reembed = source_stamp.as_ref() != Some(&desired);
    let provider = if reembed {
        Some(
            catalog::activate(
                &selection,
                &BuildContext {
                    keys: KeySources::new(identity.data().join("secrets"), None),
                    endpoint: None,
                },
                embedder.remote_opt_in,
            )
            .map_err(|error| error.message())?,
        )
    } else {
        None
    };
    let verification = transfer_verified(&source, &target, options.into_nonempty)?;
    let copied = verification.target_counts;
    let mut expected = copied.clone();
    if let Some(provider) = provider {
        reindex(&target, ActiveEmbedder::new(provider), &mut expected)?;
    }
    let final_counts = target.table_counts()?;
    verify_counts(&expected, &final_counts)?;
    let copied_source = verification.source_counts;
    let final_digests = content_digests(target.as_ref())?;
    Ok(
        serde_json::json!({"from": options.to.other(), "to": options.to, "verified": true, "reembedded": reembed, "source_unchanged": true, "source_counts": copied_source, "copied_counts": copied, "target_counts": final_counts, "config_changed": false, "source_digests": verification.source_digests, "copied_digests": verification.target_digests, "target_digests": final_digests}),
    )
}

pub fn reindex(
    target: &Arc<dyn MemStore>,
    embedder: Arc<ActiveEmbedder>,
    expected: &mut TableCounts,
) -> Result<(), String> {
    let ctx = WriteContext::system("backend-migrate-reindex");
    let identity = embedder.identity();
    if target.vector_stamp()?.is_none() {
        target.init_vectors(&ctx, &identity)?;
    }
    target.clear_shadow(&ctx, true)?;
    target.prepare_engine_switch(&ctx, &identity, 0, true)?;
    let stamp = serde_json::to_string(&identity).map_err(|error| error.to_string())?;
    let (units, _) = target.shadow_stale(&stamp)?;
    let mut vectors = 0;
    let mut blueprint_vectors = 0;
    for unit in units {
        let texts: Vec<_> = unit.texts.iter().map(|(_, _, text)| text.clone()).collect();
        let mut bytes = Vec::new();
        for batch in texts.chunks(8) {
            for result in embedder.passages(batch) {
                let vector =
                    result.map_err(|error| format!("memory_migration_reembed: {error}"))?;
                if vector.identity != identity
                    || vector.vector.len() != identity.dimension
                    || vector.vector.iter().any(|value| !value.is_finite())
                {
                    return Err(
                        "memory_migration_reembed: invalid vector stamp or dimension".into(),
                    );
                }
                for value in vector.vector {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        if unit.blueprint {
            blueprint_vectors += texts.len() as u64;
        } else {
            vectors += texts.len() as u64;
        }
        target.shadow_checkpoint(&ctx, &stamp, &unit, Some(&bytes), false, &|| true)?;
    }
    if !target.publish_shadow(&ctx, &embedder, &embedder, &stamp, &|| true)? {
        return Err("memory_migration_reembed: target changed during shadow publication".into());
    }
    expected.insert(StoreTable::Embedder, 1);
    expected.insert(StoreTable::Vectors, vectors);
    expected.insert(StoreTable::BlueprintVectors, blueprint_vectors);
    expected.insert(StoreTable::VectorPending, 0);
    if target.vector_stamp()?.as_ref() != Some(&identity) {
        return Err("memory_migration_reembed: target stamp was not published".into());
    }
    Ok(())
}
