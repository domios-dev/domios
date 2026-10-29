use super::{MemBackendError, MemBackendOpener};
use codeterm_service_core::mem::backend::MemStore;
use codeterm_types::config::MemoryBackend;
use std::sync::Arc;

pub struct MigrationClaim {
    #[cfg(feature = "mem-postgres")]
    lease: codeterm_mem_postgres::MigrationLease,
}

impl MemBackendOpener<'_> {
    pub fn migration_claim(
        &self,
        backend: MemoryBackend,
    ) -> Result<Option<MigrationClaim>, super::migrate::MigrationError> {
        if backend == MemoryBackend::Sqlite {
            return Ok(None);
        }
        let url = self
            .postgres_url
            .filter(|url| !url.trim().is_empty())
            .ok_or(MemBackendError::MissingPostgresUrl)?;
        #[cfg(feature = "mem-postgres")]
        {
            let lease = codeterm_mem_postgres::MigrationLease::try_acquire(url, Default::default())
                .map_err(|error| {
                    if error == codeterm_mem_postgres::PostgresOpenError::SchemaBusy {
                        super::migrate::MigrationError::SchemaBusy { side: backend }
                    } else {
                        super::migrate::MigrationError::Backend(MemBackendError::Open {
                            backend,
                            reason: error.to_string(),
                        })
                    }
                })?;
            Ok(Some(MigrationClaim { lease }))
        }
        #[cfg(not(feature = "mem-postgres"))]
        {
            let _ = url;
            Err(MemBackendError::PostgresNotBuilt.into())
        }
    }

    pub fn open_migration(
        &self,
        backend: MemoryBackend,
        read_only: bool,
        claim: Option<&MigrationClaim>,
    ) -> Result<Arc<dyn MemStore>, MemBackendError> {
        if backend == MemoryBackend::Sqlite {
            return if read_only {
                self.open(backend, true)
            } else {
                self.migration_target(backend)
            };
        }
        #[cfg(feature = "mem-postgres")]
        {
            claim
                .ok_or_else(|| MemBackendError::Open {
                    backend,
                    reason: "memory_migration_claim_missing".into(),
                })?
                .lease
                .open(read_only)
                .map_err(|error| MemBackendError::Open {
                    backend,
                    reason: error.to_string(),
                })
        }
        #[cfg(not(feature = "mem-postgres"))]
        {
            let _ = claim;
            Err(MemBackendError::PostgresNotBuilt)
        }
    }
}
