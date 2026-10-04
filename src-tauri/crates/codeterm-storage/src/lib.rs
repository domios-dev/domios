//! Shared typed Storage owner of one instance root's `state.db`: one writer thread,
//! at most two owned readers, typed repositories and Storage-owned assets.

mod assets;
mod backup;
mod error;
mod owner;
mod repositories;
mod schema;

pub use assets::{
    AssetAudit, AssetId, AssetRef, AssetRepository, CleanupReport, PendingCleanup, SweepReport,
};
pub use backup::{BackupReceipt, RestoreReceipt};
pub use error::{ConstraintKind, StorageError};
#[cfg(feature = "test-support")]
pub use owner::{Fault, ReentrancyProbe, WriterHold};
pub use owner::{
    Storage, StorageConfig, DEFAULT_MAILBOX_BYTES, DEFAULT_MAILBOX_COMMANDS, MAX_READ_CONNECTIONS,
};
pub use repositories::workspaces::{
    AssetRole, StoredWorkspace, WorkspaceAsset, WorkspaceId, WorkspaceInventory, WorkspaceRecord,
    WorkspaceRepository, WorkspaceUpsert, MAX_UPSERT_BATCH, MAX_WORKSPACES,
};
pub use repositories::{
    CommitReceipt, CommitTimings, Committed, Completion, Expect, Read, Revision, ShutdownReceipt,
    StorageMetrics,
};
pub use schema::{ConnectionSettings, EffectiveSettings, Synchronous};
