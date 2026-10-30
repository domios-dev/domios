use crate::executor::Transaction;
use crate::executor::{Client, Manager};
use codeterm_mem::backend::MemStore;
use parking_lot::Mutex;
use postgres::Config;
#[cfg(test)]
use postgres::NoTls;
use std::{sync::Arc, time::Duration};

mod blueprint;
mod curation;
mod engines;
mod executor;
mod fence;
mod graph;
pub use fence::MigrationLease;
mod implementation;
mod items;
mod maintenance;
mod migrations;
mod operations;
mod queries;
mod repairs;
mod transfer;
mod vectors;
mod wake_up;

pub type StoreResult<T> = Result<T, String>;

#[derive(Clone, Debug)]
pub struct PostgresOptions {
    pub schema: String,
    pub pool_size: u32,
    pub statement_timeout: Duration,
    pub read_only: bool,
}

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            schema: "domios_mem".into(),
            pool_size: 4,
            statement_timeout: Duration::from_secs(30),
            read_only: false,
        }
    }
}

impl PostgresOptions {
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresOpenError {
    InvalidOptions,
    InvalidUrl,
    Connection,
    Migration,
    SchemaBusy,
    SchemaVersionMismatch { found: Option<i64>, expected: i64 },
}

impl std::fmt::Display for PostgresOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidOptions => "invalid Postgres memory options",
            Self::InvalidUrl => "invalid Postgres memory connection secret",
            Self::Connection => "Postgres memory connection failed",
            Self::SchemaBusy => "postgres_memory_schema_busy: schema is in use",
            Self::Migration => "Postgres memory migration failed",
            Self::SchemaVersionMismatch { .. } => "Postgres memory schema version mismatch",
        })
    }
}

impl std::error::Error for PostgresOpenError {}

#[derive(Debug)]
struct SilentPoolErrors;

impl r2d2::HandleError<std::io::Error> for SilentPoolErrors {
    fn handle_error(&self, _error: std::io::Error) {}
}

#[derive(Clone)]
pub struct PostgresMemStore {
    pool: r2d2::Pool<Manager>,
    gate: Arc<Mutex<()>>,
    read_only: bool,
    fence: Arc<fence::SchemaFence>,
}

impl PostgresMemStore {
    fn connect(url: &str, options: PostgresOptions) -> Result<Self, PostgresOpenError> {
        let config = connection_config(url, &options)?;
        let fence = fence::SchemaFence::acquire(&config, &options.schema, false)?;
        Self::connect_fenced(config, options, fence)
    }

    fn connect_fenced(
        config: Config,
        options: PostgresOptions,
        fence: Arc<fence::SchemaFence>,
    ) -> Result<Self, PostgresOpenError> {
        fence.check().map_err(|_| PostgresOpenError::Connection)?;
        let pool = r2d2::Pool::builder()
            .max_size(options.pool_size)
            .min_idle(Some(0))
            .connection_timeout(Duration::from_secs(5))
            .error_handler(Box::new(SilentPoolErrors))
            .build(Manager::new(config))
            .map_err(|_| PostgresOpenError::Connection)?;
        if fence.exclusive {
            let mut conn = fence.client.lock();
            if options.read_only {
                migrations::verify_version(&mut *conn, &options.schema)?;
            } else {
                migrations::run(&mut conn, &options.schema, None)
                    .map_err(|_| PostgresOpenError::Migration)?;
            }
        } else {
            let mut conn = pool.get().map_err(|_| PostgresOpenError::Connection)?;
            if options.read_only {
                let mut tx = conn
                    .transaction()
                    .map_err(|_| PostgresOpenError::Connection)?;
                fence
                    .admit(&mut tx)
                    .map_err(|_| PostgresOpenError::Connection)?;
                migrations::verify_version(&mut tx, &options.schema)?;
                tx.commit().map_err(|_| PostgresOpenError::Connection)?;
            } else {
                migrations::run(&mut conn, &options.schema, Some(&fence))
                    .map_err(|_| PostgresOpenError::Migration)?;
            }
        }
        Ok(Self {
            pool,
            gate: Arc::new(Mutex::new(())),
            read_only: options.read_only,
            fence,
        })
    }

    fn transaction<T>(
        &self,
        f: impl FnOnce(&mut Transaction<'_>) -> StoreResult<T>,
    ) -> StoreResult<T> {
        self.transaction_error(f)
    }

    fn transaction_error<T, E: From<String>>(
        &self,
        f: impl FnOnce(&mut Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let _gate = self.gate.lock();
        self.transaction_locked(true, f)
    }

    fn transaction_locked<T, E: From<String>>(
        &self,
        wait: bool,
        f: impl FnOnce(&mut Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        if self.fence.exclusive {
            return self.transaction_on(&mut self.fence.client.lock(), wait, f);
        }
        let mut conn = self
            .pool
            .get()
            .map_err(|_| E::from("Postgres memory pool unavailable".into()))?;
        self.transaction_on(&mut conn, wait, f)
    }

    fn transaction_on<T, E: From<String>>(
        &self,
        conn: &mut Client,
        wait: bool,
        f: impl FnOnce(&mut Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut tx = conn
            .build_transaction()
            .read_only(self.read_only)
            .start()
            .map_err(|error| E::from(db_error(error)))?;
        self.fence.admit(&mut tx).map_err(E::from)?;
        if wait {
            tx.query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended(current_schema(), 0))",
                &[],
            )
            .map_err(|error| E::from(db_error(error)))?;
        } else if !tx
            .query_one(
                "SELECT pg_try_advisory_xact_lock(hashtextextended(current_schema(), 0))",
                &[],
            )
            .map_err(|error| E::from(db_error(error)))?
            .get::<_, bool>(0)
        {
            return Err(E::from(
                "memory is busy; embedder switch cannot complete now".into(),
            ));
        }
        let result = f(&mut tx)?;
        tx.commit().map_err(|error| E::from(db_error(error)))?;
        Ok(result)
    }

    fn nontransactional_write<T>(
        &self,
        f: impl FnOnce(&mut Client) -> StoreResult<T>,
    ) -> StoreResult<T> {
        let _gate = self.gate.lock();
        if self.read_only {
            return Err("postgres_memory_read_only (25006)".into());
        }
        if self.fence.exclusive {
            let mut conn = self.fence.client.lock();
            conn.simple_query("SELECT 1")
                .map_err(|_| "postgres_memory_fence_lost: reopen the store".to_string())?;
            return f(&mut conn);
        }
        let mut session = self.fence.shared_session()?;
        f(&mut session.client)
    }
}

pub fn open(url: &str, options: PostgresOptions) -> Result<Arc<dyn MemStore>, PostgresOpenError> {
    Ok(Arc::new(PostgresMemStore::connect(url, options)?))
}

#[cfg(any(test, feature = "test-support"))]
pub fn open_for_test(
    url: &str,
    options: PostgresOptions,
) -> Result<Arc<dyn MemStore>, PostgresOpenError> {
    open(url, options)
}

pub(crate) fn pg_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn db_error(error: postgres::Error) -> String {
    match error.code() {
        Some(code) if *code == postgres::error::SqlState::READ_ONLY_SQL_TRANSACTION => {
            "postgres_memory_read_only (25006)".into()
        }
        Some(code) => format!("Postgres memory operation failed ({})", code.code()),
        None => "Postgres memory operation failed".into(),
    }
}

fn valid_schema(schema: &str) -> bool {
    !schema.is_empty()
        && schema.len() <= 63
        && schema
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && schema.as_bytes()[0].is_ascii_lowercase()
        && schema != "public"
        && !schema.starts_with("pg_")
}

#[cfg(test)]
mod tests;

fn connection_config(url: &str, options: &PostgresOptions) -> Result<Config, PostgresOpenError> {
    if !valid_schema(&options.schema)
        || !(1..=32).contains(&options.pool_size)
        || options.statement_timeout < Duration::from_millis(1)
        || options.statement_timeout > Duration::from_secs(300)
    {
        return Err(PostgresOpenError::InvalidOptions);
    }
    let mut config: Config = url.parse().map_err(|_| PostgresOpenError::InvalidUrl)?;
    config.connect_timeout(Duration::from_secs(5));
    configure(&mut config, options);
    Ok(config)
}
fn configure(config: &mut Config, options: &PostgresOptions) {
    config.options(&format!(
            "-c search_path={},public -c statement_timeout={} -c lock_timeout=5000 -c default_transaction_read_only={}",
            options.schema,
            options.statement_timeout.as_millis(),
            if options.read_only { "on" } else { "off" }
        ));
}
