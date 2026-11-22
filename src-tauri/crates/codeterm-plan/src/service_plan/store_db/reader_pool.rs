use super::{db_error, PlanError};
use parking_lot::{Condvar, Mutex, MutexGuard};
use rusqlite::{Connection, OpenFlags};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::time::Duration;

const PLAN_DB_READ_POOL_SIZE: usize = 4;

/// Bounded WAL read-only connection pool so concurrent plan reads never serialize
/// behind each other or behind the single writer connection.
pub(super) struct ReaderPool {
    connections: Mutex<Vec<Connection>>,
    available: Condvar,
    #[cfg(any(test, feature = "test-support"))]
    capacity: usize,
}

impl ReaderPool {
    pub(super) fn open(
        path: &Path,
        register: impl Fn(&Connection) -> Result<(), PlanError>,
    ) -> Result<Self, PlanError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connections = Vec::with_capacity(PLAN_DB_READ_POOL_SIZE);
        for _ in 0..PLAN_DB_READ_POOL_SIZE {
            let connection = Connection::open_with_flags(path, flags).map_err(db_error)?;
            connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(db_error)?;
            connection
                .execute_batch("PRAGMA query_only = ON;")
                .map_err(db_error)?;
            register(&connection)?;
            connections.push(connection);
        }
        Ok(Self {
            #[cfg(any(test, feature = "test-support"))]
            capacity: connections.len(),
            connections: Mutex::new(connections),
            available: Condvar::new(),
        })
    }

    pub(super) fn acquire(&self) -> ReaderGuard<'_> {
        let mut connections = self.connections.lock();
        loop {
            if let Some(connection) = connections.pop() {
                return ReaderGuard {
                    pool: self,
                    connection: Some(connection),
                };
            }
            self.available.wait(&mut connections);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn acquire_all_for_test(&self) -> Vec<ReaderGuard<'_>> {
        (0..self.capacity).map(|_| self.acquire()).collect()
    }
}

pub(super) struct ReaderGuard<'a> {
    pool: &'a ReaderPool,
    connection: Option<Connection>,
}

impl Deref for ReaderGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("reader guard always holds a connection until drop")
    }
}

impl DerefMut for ReaderGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection
            .as_mut()
            .expect("reader guard always holds a connection until drop")
    }
}

impl Drop for ReaderGuard<'_> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.connections.lock().push(connection);
            self.pool.available.notify_one();
        }
    }
}

pub(super) enum ReadConnectionGuard<'a> {
    Pooled(ReaderGuard<'a>),
    Writer(MutexGuard<'a, Connection>),
}

impl Deref for ReadConnectionGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Pooled(guard) => guard,
            Self::Writer(guard) => guard,
        }
    }
}

impl DerefMut for ReadConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Pooled(guard) => guard,
            Self::Writer(guard) => guard,
        }
    }
}

#[cfg(test)]
mod tests;
