//! Shared typed Storage owner of one instance root's `state.db`: one writer thread,
//! at most two owned readers, typed repositories and Storage-owned assets.

mod assets;
mod backup;
mod error;
mod owner;
mod repositories;
mod schema;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use assets::{
    AssetAudit, AssetId, AssetRef, AssetRepository, CleanupReport, PendingCleanup, SweepReport,
};
pub use backup::{BackupReceipt, RestoreReceipt};
pub use error::{ConstraintKind, StorageError};
#[cfg(feature = "test-support")]
pub use owner::{Fault, Hold, ReentrancyProbe};
pub use owner::{
    Storage, StorageConfig, DEFAULT_MAILBOX_BYTES, DEFAULT_MAILBOX_COMMANDS, MAX_READ_CONNECTIONS,
};
pub use repositories::cron::{
    CronApplied, CronCancelReason, CronDivergenceOrigin, CronHistoryDisposition, CronHistoryImport,
    CronIdentity, CronImport, CronJobId, CronJobImport, CronJobRecord, CronJobState,
    CronLastRunDivergence, CronRepository, CronRunField, CronRunId, CronRunRecord, CronRunState,
    CronScope, CronSnapshot, CronWrite, StoredCronJob, MAX_CRON_BATCH, MAX_CRON_HISTORY,
    MAX_CRON_JOBS,
};
pub use repositories::desk::{
    DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields, DeskRepository, DeskSnapshot,
    DeskState, DeskTab, DeskTeam, DeskTeamLifecycle, DeskWorkspace, MAX_DESK_CATALOGUE,
    MAX_DESK_LAYOUT_DEPTH, MAX_DESK_LAYOUT_NODES, MAX_DESK_LEAF_BYTES, MAX_DESK_TABS,
    MAX_DESK_TEAMS, MAX_DESK_WORKSPACES,
};
pub use repositories::sentinel::{
    AmendmentRecord, BatchFence, BlueprintJournalEntry, BlueprintJournalWrite, BucketDefinition,
    CaptureRecord, CommandParameters, CommandResultRecord, CommandStatus, DefinitionProvenance,
    DefinitionUpdate, EscalateTarget, Fence, HandlerAction, HandlerDefinition, HandlerResultRecord,
    HandlerStatus, MutationOutcome, NestedClaimParent, NewTransition, ObserveKind, ObserveSource,
    RunRecord, RuntimeOutcome, SafeKind, ScheduleUpdate, SentinelCommit, SentinelCreate,
    SentinelEviction, SentinelId, SentinelIdentity, SentinelImport, SentinelRecord,
    SentinelRepository, SentinelScope, SourceKind, StageDefinition, StageRuntime, StageSource,
    StoredDefinition, StoredSentinel, TargetIdentity, TransitionRecord, MAX_SENTINEL_BATCH,
    MAX_SENTINEL_LINEAGE, MAX_SENTINEL_RECORDS, OTHER_BUCKET,
};
pub use repositories::tab_todo::{
    TabTodoAmendmentRecord, TabTodoChecklistRecord, TabTodoCountersRecord, TabTodoItemEventRecord,
    TabTodoItemRecord, TabTodoItemRefRecord, TabTodoNewItemRecord, TabTodoOwnWriteBase,
    TabTodoOwnWriteRecord, TabTodoRepository, TabTodoStateChangeRecord, MAX_TAB_TODO_AUDIT_EVENTS,
    MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_ITEM_EVENTS,
};
pub use repositories::workspaces::{
    AssetRole, StoredWorkspace, WorkspaceAsset, WorkspaceId, WorkspaceInventory, WorkspaceRecord,
    WorkspaceRepository, WorkspaceUpsert, MAX_UPSERT_BATCH, MAX_WORKSPACES,
};
pub use repositories::{
    CommitReceipt, CommitTimings, Committed, Completion, Expect, Read, Revision, ShutdownReceipt,
    StorageMetrics,
};
pub use repositories::{TeamTeardown, TeamTeardownApplied};
pub use schema::{ConnectionSettings, EffectiveSettings, Synchronous};
