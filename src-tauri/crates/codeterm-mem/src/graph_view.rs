use serde::Serialize;

use crate::store::MemAuthor;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct MemNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub importance: f64,
    pub tags: Vec<String>,
    pub has_vector: bool,
    pub has_body: bool,
    pub author: Option<MemAuthor>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct GraphFullResponse {
    pub nodes: Vec<MemNode>,
    pub edges: Vec<crate::store::GraphEdge>,
    pub stats: crate::store::GraphDumpStats,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct LabelRow {
    pub id: String,
    pub title: String,
}

#[cfg(test)]
mod tests;
