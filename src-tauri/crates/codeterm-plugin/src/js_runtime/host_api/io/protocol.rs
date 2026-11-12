use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "HostIoRequest")]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Request {
    FileExists {
        path: String,
    },
    IsSymlink {
        path: String,
    },
    ReadFile {
        path: String,
    },
    ReadFileHead {
        path: String,
        max_bytes: u64,
    },
    ReadFileTail {
        path: String,
        max_bytes: u64,
    },
    WriteFile {
        path: String,
        contents: String,
    },
    WriteFileAtomic {
        path: String,
        contents: String,
    },
    LockFile {
        path: String,
    },
    UnlockFile {
        id: String,
    },
    RemoveFile {
        path: String,
    },
    MakeDirs {
        path: String,
    },
    WriteTempFile {
        contents: String,
        suffix: Option<String>,
    },
    CanonicalPath {
        path: String,
    },
    ProjectWriteAllowed {
        root: String,
    },
    ReadDir {
        path: String,
    },
    ListChildDirs {
        path: String,
    },
    WalkDir {
        path: String,
        max_depth: usize,
    },
    SqliteQuery {
        db: String,
        sql: String,
        params: Option<Value>,
        #[serde(default)]
        nullable: bool,
    },
    JsonBooleanRecord {
        path: String,
        keys: Vec<String>,
        grant: bool,
    },
    ExecStart {
        options: Value,
    },
    ExecPoll {
        job: String,
    },
    ExecClose {
        job: String,
    },
    FetchStart {
        options: Value,
    },
    FetchStreamStart {
        options: Value,
    },
    FetchStreamPoll {
        job: String,
    },
    FetchStreamClose {
        job: String,
    },
    Progress {
        options: Value,
    },
    SurfaceCall {
        verb: String,
        args: String,
    },
    GraphEventEmit {
        event: Value,
    },
    LocalRecordingStatus,
    LocalRecordingSetEnabled {
        enabled: bool,
    },
    ReceiverStatusReport,
    ProviderVersion,
    CredentialPublic {
        id: String,
    },
    SecretGet {
        name: String,
    },
    SecretSet {
        name: String,
        value: String,
    },
    SecretDelete {
        name: String,
    },
    MemSearch {
        options: Value,
    },
    AnalyticsQuery {
        query: Value,
    },
    WorkspaceEnsure {
        options: Value,
    },
    WorkspaceTabs {
        workspace: String,
    },
    WorkerStart {
        options: Value,
    },
    WorkerPoll {
        job: String,
    },
    AgentSpawn {
        workspace: String,
        options: Value,
    },
    AgentSend {
        session: String,
        text: String,
    },
    AgentPoll {
        session: String,
    },
    AgentReap {
        session: String,
    },
    ReceiverTarget,
    ReceiverTargetClear,
    #[cfg(test)]
    TestGate {
        entered: String,
        release: String,
        completed: String,
    },
}

impl Request {
    pub(super) fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::FileExists { .. }
                | Self::IsSymlink { .. }
                | Self::ReadFile { .. }
                | Self::ReadFileHead { .. }
                | Self::ReadFileTail { .. }
                | Self::CanonicalPath { .. }
                | Self::ProjectWriteAllowed { .. }
                | Self::ReadDir { .. }
                | Self::ListChildDirs { .. }
                | Self::WalkDir { .. }
                | Self::SqliteQuery { .. }
                | Self::ProviderVersion
                | Self::CredentialPublic { .. }
                | Self::SecretGet { .. }
                | Self::MemSearch { .. }
                | Self::AnalyticsQuery { .. }
                | Self::WorkspaceTabs { .. }
                | Self::WorkerPoll { .. }
                | Self::ExecPoll { .. }
                | Self::LocalRecordingStatus
                | Self::ReceiverStatusReport
                | Self::AgentPoll { .. }
                | Self::ReceiverTarget
                | Self::JsonBooleanRecord { grant: false, .. }
        )
    }
}
