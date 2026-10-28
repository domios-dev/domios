use codeterm_service_core::{
    mem::backend::{MemStore, SqliteMemStore},
    secrets,
};
use codeterm_types::config::MemoryBackend;
use std::{path::Path, sync::Arc};

pub const POSTGRES_URL_KEY: &str = "domios.mem:postgres_url";
pub mod migrate;

pub fn open_configured(
    data_dir: &Path,
    config: &codeterm_types::config::schema::AppConfig,
    registry: &Arc<secrets::SecretRegistry>,
) -> Result<Arc<dyn MemStore>, MemBackendError> {
    if config.memory.backend == MemoryBackend::Sqlite {
        let store = MemBackendOpener {
            data_dir,
            postgres_url: None,
        }
        .open(MemoryBackend::Sqlite, false)?;
        if store
            .table_counts()
            .map_err(|reason| MemBackendError::Open {
                backend: MemoryBackend::Sqlite,
                reason,
            })?[&codeterm_service_core::mem::backend::StoreTable::Items]
            > 0
        {
            return Ok(store);
        }
    }
    let url = postgres_url(secrets::build_active_store(
        &config.integrations.secrets,
        registry,
    ))?;
    MemBackendOpener {
        data_dir,
        postgres_url: url.as_ref().map(|url| url.expose_secret()),
    }
    .guarded_open(config.memory.backend)
}

#[derive(Debug, PartialEq, Eq)]
pub enum MemBackendError {
    MissingPostgresUrl,
    PostgresNotBuilt,
    SecretUnavailable,
    Open {
        backend: MemoryBackend,
        reason: String,
    },
    EmptySelection {
        selected: MemoryBackend,
        other: MemoryBackend,
    },
}

impl std::fmt::Display for MemBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPostgresUrl => write!(f, "memory_postgres_url_missing: secret {POSTGRES_URL_KEY} is missing or empty"),
            Self::PostgresNotBuilt => f.write_str("memory_postgres_not_built: this build has no Postgres support"),
            Self::SecretUnavailable => write!(f, "memory_secret_unavailable: cannot read {POSTGRES_URL_KEY}"),
            Self::Open { backend, reason } => write!(f, "memory_backend_open: {backend}: {reason}"),
            Self::EmptySelection { selected, other } => write!(f,
                "memory_backend_empty: selected {selected} is empty while {other} has items; stop the daemon and run domios mem backend migrate --to {selected}"),
        }
    }
}
impl std::error::Error for MemBackendError {}

pub fn startup_decision(
    selected: MemoryBackend,
    selected_items: u64,
    other_items: Option<u64>,
) -> Result<(), MemBackendError> {
    if selected_items == 0 && other_items.is_some_and(|count| count > 0) {
        return Err(MemBackendError::EmptySelection {
            selected,
            other: selected.other(),
        });
    }
    Ok(())
}

pub struct MemBackendOpener<'a> {
    pub data_dir: &'a Path,
    pub postgres_url: Option<&'a str>,
}

impl MemBackendOpener<'_> {
    pub fn migration_target(
        &self,
        backend: MemoryBackend,
    ) -> Result<Arc<dyn MemStore>, MemBackendError> {
        if backend == MemoryBackend::Sqlite {
            let mem = self.data_dir.join("mem");
            return SqliteMemStore::open_for_import(&mem.join("knowledge.db"), &mem.join("blobs"))
                .map(|store| Arc::new(store) as Arc<dyn MemStore>)
                .map_err(|reason| MemBackendError::Open { backend, reason });
        }
        self.open(backend, false)
    }
    pub fn open(
        &self,
        backend: MemoryBackend,
        read_only: bool,
    ) -> Result<Arc<dyn MemStore>, MemBackendError> {
        let failed = |reason| MemBackendError::Open { backend, reason };
        match backend {
            MemoryBackend::Sqlite => {
                let mem = self.data_dir.join("mem");
                let store = if read_only {
                    SqliteMemStore::open_read_only(&mem.join("knowledge.db"), &mem.join("blobs"))
                        .map_err(|error| failed(error.to_string()))?
                } else {
                    SqliteMemStore::open(&mem.join("knowledge.db"), &mem.join("blobs"))
                        .map_err(failed)?
                };
                Ok(Arc::new(store))
            }
            MemoryBackend::Postgres => {
                let url = self
                    .postgres_url
                    .filter(|url| !url.trim().is_empty())
                    .ok_or(MemBackendError::MissingPostgresUrl)?;
                #[cfg(feature = "mem-postgres")]
                {
                    use codeterm_mem_postgres::PostgresOptions;
                    let options = if read_only {
                        PostgresOptions::default().read_only()
                    } else {
                        PostgresOptions::default()
                    };
                    codeterm_mem_postgres::open(url, options)
                        .map_err(|error| failed(error.to_string()))
                }
                #[cfg(not(feature = "mem-postgres"))]
                {
                    let _ = url;
                    Err(MemBackendError::PostgresNotBuilt)
                }
            }
        }
    }

    pub fn guarded_open(
        &self,
        selected: MemoryBackend,
    ) -> Result<Arc<dyn MemStore>, MemBackendError> {
        let store = self.open(selected, false)?;
        let items = store
            .table_counts()
            .map_err(|reason| MemBackendError::Open {
                backend: selected,
                reason,
            })?[&codeterm_service_core::mem::backend::StoreTable::Items];
        if items == 0 {
            let other = selected.other();
            let configured = match other {
                MemoryBackend::Sqlite => self
                    .data_dir
                    .join("mem/knowledge.db")
                    .try_exists()
                    .map_err(|error| MemBackendError::Open {
                        backend: other,
                        reason: error.to_string(),
                    })?,
                MemoryBackend::Postgres => self.postgres_url.is_some(),
            };
            if configured {
                let other_store = self.open(other, true)?;
                let other_items =
                    other_store
                        .table_counts()
                        .map_err(|reason| MemBackendError::Open {
                            backend: other,
                            reason,
                        })?[&codeterm_service_core::mem::backend::StoreTable::Items];
                startup_decision(selected, items, Some(other_items))?;
            }
        }
        Ok(store)
    }
}

pub fn postgres_url(
    store: Arc<dyn secrets::SecretStore>,
) -> Result<Option<secrets::SecretString>, MemBackendError> {
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| MemBackendError::SecretUnavailable)?;
                runtime.block_on(async move {
                    tokio::time::timeout(std::time::Duration::from_secs(10), async {
                        store
                            .init()
                            .await
                            .map_err(|_| MemBackendError::SecretUnavailable)?;
                        match store.get(POSTGRES_URL_KEY).await {
                            Ok(value) => Ok(Some(value.value)),
                            Err(secrets::SecretError::NotFound) => Ok(None),
                            Err(_) => Err(MemBackendError::SecretUnavailable),
                        }
                    })
                    .await
                    .map_err(|_| MemBackendError::SecretUnavailable)?
                })
            })
            .join()
    })
    .map_err(|_| MemBackendError::SecretUnavailable)?
}

#[cfg(test)]
mod tests;
