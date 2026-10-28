use crate::PostgresMemStore;
use codeterm_blueprint_types::{
    blueprint_document::{
        calls::{projection::ExpandedGraph, CallPin, Drift, LocalCallee},
        Document,
    },
    blueprint_memory::{CuratorEvidence, Entry},
};
use codeterm_mem::backend::{blueprint::*, WriteContext};
use codeterm_mem::blueprint_library::{
    index::{Enrichment, IndexRow, VectorKey, Vectors},
    qualification::PreparedBlueprint,
    recall::Candidate,
    SaveResult,
};
use codeterm_mem::embedder::{ActiveEmbedder, EmbedderIdentity, StampedVector};
use codeterm_mem::solution_metadata::SolutionMetadata;
use codeterm_mem::{NewMemItem, SearchQuery};

impl BlueprintStore for PostgresMemStore {
    fn blueprint_save(
        &self,
        _ctx: &WriteContext,
        _area: &str,
        _prepared: PreparedBlueprint,
        _merge: bool,
        _area_cap: usize,
        _purpose: Option<Purpose>,
        _embedder: &ActiveEmbedder,
    ) -> Result<SaveResult, String> {
        Err(crate::unsupported("blueprint_save"))
    }
    fn blueprint_entries(
        &self,
        _ctx: &WriteContext,
        _area: &str,
        _validate: bool,
        _team: Option<&str>,
    ) -> Result<Vec<Entry>, String> {
        Err(crate::unsupported("blueprint_entries"))
    }
    fn blueprint_candidates(
        &self,
        _ctx: &WriteContext,
        _area: &str,
        _team: Option<&str>,
        _identity: Option<&EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String> {
        Err(crate::unsupported("blueprint_candidates"))
    }
    fn blueprint_readonly(&self, _area: &str) -> Result<Vec<Entry>, String> {
        Err(crate::unsupported("blueprint_readonly"))
    }
    fn blueprint_archive(&self, _ctx: &WriteContext, _area: &str, _id: &str) -> Result<(), String> {
        Err(crate::unsupported("blueprint_archive"))
    }
    fn blueprint_reuse(&self, _ctx: &WriteContext, _ids: &[String]) -> Result<(), String> {
        Err(crate::unsupported("blueprint_reuse"))
    }
    fn blueprint_dream(
        &self,
        _ctx: &WriteContext,
        _embedder: &ActiveEmbedder,
        _turn: u64,
    ) -> Result<BlueprintChanges, String> {
        Err(crate::unsupported("blueprint_dream"))
    }
    fn blueprint_solution_show(&self, _area: &str, _id: &str) -> Result<ExpandedGraph, String> {
        Err(crate::unsupported("blueprint_solution_show"))
    }
    fn blueprint_pin_calls(
        &self,
        _ctx: &WriteContext,
        _area: &str,
        _team: &str,
        _document: Document,
    ) -> Result<Document, String> {
        Err(crate::unsupported("blueprint_pin_calls"))
    }
    fn blueprint_pin_bundle(
        &self,
        _ctx: &WriteContext,
        _area: &str,
        _team: &str,
        _document: Document,
        _local: &[LocalCallee],
    ) -> Result<Document, String> {
        Err(crate::unsupported("blueprint_pin_bundle"))
    }
    fn blueprint_call_head(&self, _area: &str, _team: &str, _id: &str) -> Result<Document, String> {
        Err(crate::unsupported("blueprint_call_head"))
    }
    fn blueprint_call_revision(
        &self,
        _area: &str,
        _team: &str,
        _pin: &CallPin,
    ) -> Result<Document, String> {
        Err(crate::unsupported("blueprint_call_revision"))
    }
    fn blueprint_call_head_for_drift(
        &self,
        _area: &str,
        _team: &str,
        _id: &str,
    ) -> Result<(Document, Option<String>), String> {
        Err(crate::unsupported("blueprint_call_head_for_drift"))
    }
    fn blueprint_call_drift(
        &self,
        _area: &str,
        _team: &str,
        _document: Document,
    ) -> Result<Vec<Drift>, String> {
        Err(crate::unsupported("blueprint_call_drift"))
    }
    fn blueprint_index(&self, _id: &str) -> Result<Option<IndexRow>, String> {
        Err(crate::unsupported("blueprint_index"))
    }
    fn blueprint_index_snapshot(&self, _id: &str) -> Result<(IndexRow, String), String> {
        Err(crate::unsupported("blueprint_index_snapshot"))
    }
    fn blueprint_complete_enrichment(
        &self,
        _ctx: &WriteContext,
        _id: &str,
        _document_hash: &str,
        _enrichment: Enrichment,
    ) -> Result<bool, String> {
        Err(crate::unsupported("blueprint_complete_enrichment"))
    }
    fn blueprint_indexed_items(&self) -> Result<Vec<String>, String> {
        Err(crate::unsupported("blueprint_indexed_items"))
    }
    fn blueprint_vector_projection(
        &self,
        _id: &str,
    ) -> Result<Option<(IndexRow, Vec<String>)>, String> {
        Err(crate::unsupported("blueprint_vector_projection"))
    }
    fn blueprint_vectors(
        &self,
        _id: &str,
        _identity: &EmbedderIdentity,
    ) -> Result<Vectors, String> {
        Err(crate::unsupported("blueprint_vectors"))
    }
    fn blueprint_pair(
        &self,
        _a: &str,
        _b: &str,
        _embedder: &ActiveEmbedder,
    ) -> Result<(IndexedVectors, IndexedVectors), String> {
        Err(crate::unsupported("blueprint_pair"))
    }
    fn blueprint_replace_vectors(
        &self,
        _ctx: &WriteContext,
        _id: &str,
        _embedder: &ActiveEmbedder,
        _identity: &EmbedderIdentity,
        _keep: &[VectorKey],
        _written: &[(VectorKey, String, Vec<f32>)],
    ) -> Result<(), String> {
        Err(crate::unsupported("blueprint_replace_vectors"))
    }
    fn solution_candidates(
        &self,
        _query: &SearchQuery,
        _source: Option<&str>,
        _embedding: Option<&StampedVector>,
    ) -> Result<SolutionCandidates, String> {
        Err(crate::unsupported("solution_candidates"))
    }
    fn solution_metadata(&self, _id: &str) -> Result<Option<SolutionMetadata>, String> {
        Err(crate::unsupported("solution_metadata"))
    }
    fn curator_skill_save(
        &self,
        _ctx: &WriteContext,
        _item: NewMemItem,
        _improve: Option<&str>,
        _proof: CuratorEvidence,
        _now: i64,
    ) -> Result<String, String> {
        Err(crate::unsupported("curator_skill_save"))
    }
}
