use super::WriteContext;
pub use crate::blueprint_library::merge_policy::Purpose;
use crate::blueprint_library::{
    index::{Enrichment, IndexRow, VectorKey, Vectors},
    qualification::PreparedBlueprint,
    recall::Candidate,
    SaveResult,
};
use crate::embedder::{ActiveEmbedder, EmbedderIdentity, StampedVector};
use crate::solution_metadata::SolutionMetadata;
use crate::{NewMemItem, SearchHit, SearchQuery};
use codeterm_blueprint_types::{
    blueprint_document::{
        calls::{projection::ExpandedGraph, CallPin, Drift, LocalCallee},
        Document,
    },
    blueprint_memory::{CuratorEvidence, Entry},
};
use std::collections::BTreeSet;

pub struct BlueprintChanges {
    pub ids: Vec<String>,
    pub areas: Vec<String>,
}

pub type IndexedVectors = (IndexRow, Vectors);
pub type SolutionCandidates = (Vec<SearchHit>, BTreeSet<String>);

pub trait BlueprintStore: Send + Sync {
    fn blueprint_save(
        &self,
        ctx: &WriteContext,
        area: &str,
        prepared: PreparedBlueprint,
        merge: bool,
        area_cap: usize,
        purpose: Option<Purpose>,
        embedder: &ActiveEmbedder,
    ) -> Result<SaveResult, String>;
    fn blueprint_entries(
        &self,
        ctx: &WriteContext,
        area: &str,
        validate: bool,
        team: Option<&str>,
    ) -> Result<Vec<Entry>, String>;
    fn blueprint_candidates(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: Option<&str>,
        identity: Option<&EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String>;
    fn blueprint_readonly(&self, area: &str) -> Result<Vec<Entry>, String>;
    fn blueprint_archive(&self, ctx: &WriteContext, area: &str, id: &str) -> Result<(), String>;
    fn blueprint_reuse(&self, ctx: &WriteContext, ids: &[String]) -> Result<(), String>;
    fn blueprint_dream(
        &self,
        ctx: &WriteContext,
        embedder: &ActiveEmbedder,
        turn: u64,
    ) -> Result<BlueprintChanges, String>;
    fn blueprint_solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String>;
    fn blueprint_pin_calls(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Document, String>;
    fn blueprint_pin_bundle(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
        local: &[LocalCallee],
    ) -> Result<Document, String>;
    fn blueprint_call_head(&self, area: &str, team: &str, id: &str) -> Result<Document, String>;
    fn blueprint_call_revision(
        &self,
        area: &str,
        team: &str,
        pin: &CallPin,
    ) -> Result<Document, String>;
    fn blueprint_call_head_for_drift(
        &self,
        area: &str,
        team: &str,
        id: &str,
    ) -> Result<(Document, Option<String>), String>;
    fn blueprint_call_drift(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Vec<Drift>, String>;
    fn blueprint_index(&self, id: &str) -> Result<Option<IndexRow>, String>;
    fn blueprint_index_snapshot(&self, id: &str) -> Result<(IndexRow, String), String>;
    fn blueprint_complete_enrichment(
        &self,
        ctx: &WriteContext,
        id: &str,
        document_hash: &str,
        enrichment: Enrichment,
    ) -> Result<bool, String>;
    fn blueprint_indexed_items(&self) -> Result<Vec<String>, String>;
    fn blueprint_vector_projection(
        &self,
        id: &str,
    ) -> Result<Option<(IndexRow, Vec<String>)>, String>;
    fn blueprint_vectors(&self, id: &str, identity: &EmbedderIdentity) -> Result<Vectors, String>;
    fn blueprint_pair(
        &self,
        a: &str,
        b: &str,
        embedder: &ActiveEmbedder,
    ) -> Result<(IndexedVectors, IndexedVectors), String>;
    fn blueprint_replace_vectors(
        &self,
        ctx: &WriteContext,
        id: &str,
        embedder: &ActiveEmbedder,
        identity: &EmbedderIdentity,
        keep: &[VectorKey],
        written: &[(VectorKey, String, Vec<f32>)],
    ) -> Result<(), String>;
    fn solution_candidates(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
        embedding: Option<&StampedVector>,
    ) -> Result<SolutionCandidates, String>;
    fn solution_metadata(&self, id: &str) -> Result<Option<SolutionMetadata>, String>;
    fn curator_skill_save(
        &self,
        ctx: &WriteContext,
        item: NewMemItem,
        improve: Option<&str>,
        proof: CuratorEvidence,
        now: i64,
    ) -> Result<String, String>;
}
