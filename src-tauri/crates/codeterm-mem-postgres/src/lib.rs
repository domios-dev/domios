use codeterm_mem::backend::MemStore;
use parking_lot::Mutex;
use postgres::{Config, NoTls, Transaction};
use r2d2_postgres::PostgresConnectionManager;
use std::{sync::Arc, time::Duration};

mod blueprint;
mod curation;
mod engines;
mod graph;
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
type Manager = PostgresConnectionManager<NoTls>;

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
    SchemaVersionMismatch { found: Option<i64>, expected: i64 },
}

impl std::fmt::Display for PostgresOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidOptions => "invalid Postgres memory options",
            Self::InvalidUrl => "invalid Postgres memory connection secret",
            Self::Connection => "Postgres memory connection failed",
            Self::Migration => "Postgres memory migration failed",
            Self::SchemaVersionMismatch { .. } => "Postgres memory schema version mismatch",
        })
    }
}

impl std::error::Error for PostgresOpenError {}

#[derive(Debug)]
struct SilentPoolErrors;

impl r2d2::HandleError<postgres::Error> for SilentPoolErrors {
    fn handle_error(&self, _error: postgres::Error) {}
}

#[derive(Clone)]
pub struct PostgresMemStore {
    pool: r2d2::Pool<Manager>,
    gate: Arc<Mutex<()>>,
    read_only: bool,
}

impl PostgresMemStore {
    fn connect(url: &str, options: PostgresOptions) -> Result<Self, PostgresOpenError> {
        if !valid_schema(&options.schema)
            || !(1..=32).contains(&options.pool_size)
            || options.statement_timeout < Duration::from_millis(1)
            || options.statement_timeout > Duration::from_secs(300)
        {
            return Err(PostgresOpenError::InvalidOptions);
        }
        let mut config: Config = url.parse().map_err(|_| PostgresOpenError::InvalidUrl)?;
        config.connect_timeout(Duration::from_secs(5));
        config.options(&format!(
            "-c search_path={},public -c statement_timeout={} -c lock_timeout=5000 -c default_transaction_read_only={}",
            options.schema,
            options.statement_timeout.as_millis(),
            if options.read_only { "on" } else { "off" }
        ));
        let pool = r2d2::Pool::builder()
            .max_size(options.pool_size)
            .min_idle(Some(0))
            .connection_timeout(Duration::from_secs(5))
            .error_handler(Box::new(SilentPoolErrors))
            .build(PostgresConnectionManager::new(config, NoTls))
            .map_err(|_| PostgresOpenError::Connection)?;
        let mut conn = pool.get().map_err(|_| PostgresOpenError::Connection)?;
        if options.read_only {
            migrations::verify_version(&mut conn, &options.schema)?;
        } else {
            migrations::run(&mut conn, &options.schema)
                .map_err(|_| PostgresOpenError::Migration)?;
        }
        drop(conn);
        Ok(Self {
            pool,
            gate: Arc::new(Mutex::new(())),
            read_only: options.read_only,
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
        let mut conn = self
            .pool
            .get()
            .map_err(|_| E::from("Postgres memory pool unavailable".into()))?;
        let mut tx = conn
            .transaction()
            .map_err(|error| E::from(db_error(error)))?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended(current_schema(), 0))",
            &[],
        )
        .map_err(|error| E::from(db_error(error)))?;
        let result = f(&mut tx)?;
        tx.commit().map_err(|error| E::from(db_error(error)))?;
        Ok(result)
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
