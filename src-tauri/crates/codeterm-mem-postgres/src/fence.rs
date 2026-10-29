use crate::{PostgresMemStore, PostgresOpenError, PostgresOptions, StoreResult};
use parking_lot::Mutex;
use postgres::{Client, Config, NoTls};
use std::sync::Arc;

pub(crate) struct SchemaFence {
    pub(crate) client: Mutex<Client>,
    pub(crate) key: String,
    pub(crate) exclusive: bool,
    config: Config,
}

pub(crate) struct SessionLease {
    pub(crate) client: Client,
    key: String,
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        let _ = self.client.query_one(
            "SELECT pg_advisory_unlock_shared(hashtextextended($1, 0))",
            &[&self.key],
        );
    }
}

impl Drop for SchemaFence {
    fn drop(&mut self) {
        let sql = if self.exclusive {
            "SELECT pg_advisory_unlock(hashtextextended($1, 0))"
        } else {
            "SELECT pg_advisory_unlock_shared(hashtextextended($1, 0))"
        };
        let _ = self.client.get_mut().query_one(sql, &[&self.key]);
    }
}

impl SchemaFence {
    pub(crate) fn acquire(
        config: &Config,
        schema: &str,
        exclusive: bool,
    ) -> Result<Arc<Self>, PostgresOpenError> {
        let mut client = config
            .connect(NoTls)
            .map_err(|_| PostgresOpenError::Connection)?;
        let key = format!("domios_mem:schema:{schema}");
        let sql = if exclusive {
            "SELECT pg_try_advisory_lock(hashtextextended($1, 0))"
        } else {
            "SELECT pg_try_advisory_lock_shared(hashtextextended($1, 0))"
        };
        let acquired: bool = client
            .query_one(sql, &[&key])
            .map_err(|_| PostgresOpenError::Connection)?
            .get(0);
        if !acquired {
            return Err(PostgresOpenError::SchemaBusy);
        }
        Ok(Arc::new(Self {
            client: Mutex::new(client),
            key,
            exclusive,
            config: config.clone(),
        }))
    }

    pub(crate) fn admit(&self, tx: &mut postgres::Transaction<'_>) -> StoreResult<()> {
        if self.exclusive {
            return Ok(());
        }
        let acquired: bool = tx
            .query_one(
                "SELECT pg_try_advisory_xact_lock_shared(hashtextextended($1, 0))",
                &[&self.key],
            )
            .map_err(crate::db_error)?
            .get(0);
        if !acquired {
            return Err("postgres_memory_schema_busy".into());
        }
        self.check()
    }

    pub(crate) fn shared_session(&self) -> StoreResult<SessionLease> {
        // A dedicated client keeps session lease ownership independent of pool recycling.
        let mut client = self.config.connect(NoTls).map_err(crate::db_error)?;
        let acquired: bool = client
            .query_one(
                "SELECT pg_try_advisory_lock_shared(hashtextextended($1, 0))",
                &[&self.key],
            )
            .map_err(crate::db_error)?
            .get(0);
        if !acquired {
            return Err("postgres_memory_schema_busy".into());
        }
        let session = SessionLease {
            client,
            key: self.key.clone(),
        };
        self.check()?;
        Ok(session)
    }

    pub(crate) fn check(&self) -> StoreResult<()> {
        self.client
            .lock()
            .simple_query("SELECT 1")
            .map_err(|_| "postgres_memory_fence_lost: reopen the store".to_string())?;
        Ok(())
    }
}

pub struct MigrationLease {
    fence: Arc<SchemaFence>,
    config: Config,
    options: PostgresOptions,
}

impl MigrationLease {
    /// Acquire source, then target, then bootstrap during open; never acquire a daemon lease.
    pub fn try_acquire(url: &str, options: PostgresOptions) -> Result<Self, PostgresOpenError> {
        let config = crate::connection_config(url, &options)?;
        let fence = SchemaFence::acquire(&config, &options.schema, true)?;
        Ok(Self {
            fence,
            config,
            options,
        })
    }

    pub fn open(
        &self,
        read_only: bool,
    ) -> Result<Arc<dyn codeterm_mem::backend::MemStore>, PostgresOpenError> {
        let mut options = self.options.clone();
        options.read_only = read_only;
        let mut config = self.config.clone();
        crate::configure(&mut config, &options);
        Ok(Arc::new(PostgresMemStore::connect_fenced(
            config,
            options,
            self.fence.clone(),
        )?))
    }
}
