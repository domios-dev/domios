use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StoreTable {
    Items,
    History,
    Entities,
    Relations,
    Aliases,
    Pins,
    Fts,
    Embedder,
    Vectors,
    SolutionMetadata,
    BlueprintCallScopes,
    BlueprintCallRevisions,
    BlueprintIndex,
    BlueprintVectors,
    BlueprintCleanupReceipt,
    SourceTombstones,
    PrecompactPurges,
    PrecompactBlobs,
    VectorPending,
    LessonAudit,
    GraphPurgeLog,
}

impl StoreTable {
    pub const ALL: [Self; 21] = [
        Self::Items,
        Self::History,
        Self::Entities,
        Self::Relations,
        Self::Aliases,
        Self::Pins,
        Self::Fts,
        Self::Embedder,
        Self::Vectors,
        Self::SolutionMetadata,
        Self::BlueprintCallScopes,
        Self::BlueprintCallRevisions,
        Self::BlueprintIndex,
        Self::BlueprintVectors,
        Self::BlueprintCleanupReceipt,
        Self::SourceTombstones,
        Self::PrecompactPurges,
        Self::PrecompactBlobs,
        Self::VectorPending,
        Self::LessonAudit,
        Self::GraphPurgeLog,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StoreValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreRow {
    pub values: BTreeMap<String, StoreValue>,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportCursor {
    pub table: StoreTable,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportPage {
    pub table: StoreTable,
    pub rows: Vec<StoreRow>,
    pub next: Option<ExportCursor>,
}

pub type TableCounts = BTreeMap<StoreTable, u64>;

pub trait ExportSnapshot {
    fn export_page(&self, after: Option<&ExportCursor>, limit: usize)
        -> Result<ExportPage, String>;
    fn table_counts(&self) -> Result<TableCounts, String>;
}
