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
    SourceTombstones,
    PrecompactPurges,
    PrecompactBlobs,
}

impl StoreTable {
    pub const ALL: [Self; 13] = [
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
        Self::SourceTombstones,
        Self::PrecompactPurges,
        Self::PrecompactBlobs,
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
