use crate::{PostgresMemStore, PostgresOpenError, PostgresOptions, StoreResult};
use parking_lot::Mutex;
use postgres::{Client, Config, NoTls};
use std::sync::Arc;

pub(crate) struct SchemaFence {
    pub(crate) client: Mutex<Client>,
    pub(crate) key: String,
    pub(crate) exclusive: bool,
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
        }))
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
