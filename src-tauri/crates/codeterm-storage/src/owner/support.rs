//! Test hooks. Without the `test-support` feature every hook compiles to a no-op.

use crate::assets::AssetId;
#[cfg(feature = "test-support")]
pub use api::{Fault, Hold, ReentrancyProbe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaultPoint {
    Commit,
    AssetPublish,
    Cleanup,
}

#[derive(Debug, Default)]
pub(crate) struct Faults {
    #[cfg(feature = "test-support")]
    armed: Vec<FaultPoint>,
    #[cfg(feature = "test-support")]
    next_asset_id: Option<AssetId>,
}

impl Faults {
    pub(crate) fn take(&mut self, point: FaultPoint) -> bool {
        #[cfg(feature = "test-support")]
        if let Some(index) = self.armed.iter().position(|armed| *armed == point) {
            self.armed.remove(index);
            return true;
        }
        let _ = point;
        false
    }

    pub(crate) fn take_asset_id(&mut self) -> Option<AssetId> {
        #[cfg(feature = "test-support")]
        return self.next_asset_id.take();
        #[cfg(not(feature = "test-support"))]
        None
    }
}

/// Pause points inside production read paths.
#[derive(Default)]
pub(crate) struct ReadGates {
    #[cfg(feature = "test-support")]
    asset_file: std::sync::Mutex<Option<api::Gate>>,
}

impl ReadGates {
    pub(crate) fn before_asset_file(&self) {
        #[cfg(feature = "test-support")]
        {
            let gate = super::lock(&self.asset_file).take();
            if let Some(gate) = gate {
                gate.block();
            }
        }
    }
}

#[cfg(feature = "test-support")]
mod api {
    use super::super::{lock, Storage, StorageConfig, WriterEnv};
    use super::FaultPoint;
    use crate::assets::{AssetId, Layout};
    use crate::error::{sqlite_error, StorageError};
    use crate::repositories::Completion;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Fault {
        FailNextCommit,
        FailNextAssetPublish,
        FailNextCleanup,
        /// The next published asset reuses this id, exercising no-replace publication.
        NextAssetId(AssetId),
        /// Caps the database at its current page count so growth fails with SQLITE_FULL.
        DatabaseFull,
        ClearDatabaseFull,
    }

    /// Keeps an owner thread at a pause point until released or dropped.
    pub struct Hold {
        held: mpsc::Receiver<()>,
        release: Option<mpsc::Sender<()>>,
    }

    impl Hold {
        pub fn wait_held(&self, deadline: Duration) -> Result<(), StorageError> {
            self.held
                .recv_timeout(deadline)
                .map_err(|_| StorageError::Unconfirmed {
                    operation: "owner hold",
                })
        }

        pub fn release(mut self) {
            self.release.take();
        }
    }

    /// What each Storage entry point returned when called from the writer thread.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ReentrancyProbe {
        pub read: Option<StorageError>,
        pub wait: Option<StorageError>,
        pub shutdown: Option<StorageError>,
        pub open: Option<StorageError>,
    }

    pub(crate) struct Gate {
        held: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl Gate {
        pub(crate) fn block(self) {
            let _ = self.held.send(());
            let _ = self.release.recv();
        }
    }

    fn gate() -> (Gate, Hold) {
        let (held_tx, held) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let gate = Gate {
            held: held_tx,
            release: release_rx,
        };
        let hold = Hold {
            held,
            release: Some(release),
        };
        (gate, hold)
    }

    fn admitted<T>(hold: Hold, queued: Completion<T>) -> Result<Hold, StorageError> {
        match queued.try_take() {
            Ok(Err(error)) => Err(error),
            _ => Ok(hold),
        }
    }

    impl Storage {
        /// Holds the writer between transactions.
        pub fn hold_writer(&self) -> Result<Hold, StorageError> {
            let (gate, hold) = gate();
            let queued = self.submit_exclusive(0, move |_| {
                gate.block();
                Ok(())
            });
            admitted(hold, queued)
        }

        /// Holds the writer inside an open IMMEDIATE transaction holding an uncommitted change
        /// to every workspace name; the change rolls back on release.
        pub fn hold_write_transaction(&self) -> Result<Hold, StorageError> {
            let (gate, hold) = gate();
            let queued = self.submit_mutation(0, move |cx| -> Result<(), StorageError> {
                cx.write("UPDATE workspaces SET name = name || ' (uncommitted)'", [])?;
                gate.block();
                Err(StorageError::Injected {
                    point: "held transaction",
                })
            });
            admitted(hold, queued)
        }

        /// Pauses the next asset read inside its read snapshot, after the committed reference
        /// was found and before the file is opened.
        pub fn pause_next_asset_read(&self) -> Hold {
            let (gate, hold) = gate();
            *lock(&self.shared.gates.asset_file) = Some(gate);
            hold
        }

        pub fn inject_fault(&self, fault: Fault) -> Completion<()> {
            self.submit_exclusive(0, move |env| apply_fault(env, fault))
        }

        pub fn probe_reentrancy(&self, other_root: &Path) -> Completion<ReentrancyProbe> {
            let storage = self.clone();
            let other_root = other_root.to_path_buf();
            self.submit_exclusive(0, move |_| {
                let first_error =
                    |completion: Completion<_>| completion.try_take().ok().and_then(Result::err);
                Ok(ReentrancyProbe {
                    read: first_error(storage.workspaces().list()),
                    wait: storage
                        .committed_after(storage.revision())
                        .wait(Duration::from_secs(5))
                        .err(),
                    shutdown: storage.shutdown().try_take().ok().and_then(Result::err),
                    open: Storage::open(&other_root, StorageConfig::default()).err(),
                })
            })
        }

        pub fn database_path(root: &Path) -> PathBuf {
            Layout::of_root(root).database
        }

        pub fn lock_path(root: &Path) -> PathBuf {
            Layout::of_root(root).lock
        }
    }

    fn apply_fault(env: &mut WriterEnv<'_>, fault: Fault) -> Result<(), StorageError> {
        match fault {
            Fault::FailNextCommit => env.faults.armed.push(FaultPoint::Commit),
            Fault::FailNextAssetPublish => env.faults.armed.push(FaultPoint::AssetPublish),
            Fault::FailNextCleanup => env.faults.armed.push(FaultPoint::Cleanup),
            Fault::NextAssetId(asset) => env.faults.next_asset_id = Some(asset),
            Fault::DatabaseFull => {
                let pages: i64 = env
                    .conn
                    .query_row("PRAGMA page_count", [], |row| row.get(0))
                    .map_err(|e| sqlite_error("read page count", e))?;
                set_page_limit(env.conn, pages)?;
            }
            Fault::ClearDatabaseFull => set_page_limit(env.conn, 4_294_967_294)?,
        }
        Ok(())
    }

    fn set_page_limit(conn: &Connection, pages: i64) -> Result<(), StorageError> {
        let _: i64 = conn
            .query_row(&format!("PRAGMA max_page_count = {pages}"), [], |row| {
                row.get(0)
            })
            .map_err(|e| sqlite_error("set page limit", e))?;
        Ok(())
    }
}
