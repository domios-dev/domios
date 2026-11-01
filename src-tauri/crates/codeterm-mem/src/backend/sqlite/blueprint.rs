use super::SqliteMemStore;
use crate::backend::{blueprint::*, WriteContext};
use crate::blueprint_library::{
    index::{Enrichment, IndexRow, VectorKey, Vectors},
    qualification::PreparedBlueprint,
    recall::Candidate,
    SaveResult, SOURCE,
};
use crate::embedder::{ActiveEmbedder, EmbedderIdentity, StampedVector};
use crate::{store, NewMemItem, SearchQuery};
use codeterm_blueprint_types::{
    blueprint_document::{
        calls::{projection::ExpandedGraph, CallPin, Drift, LocalCallee, MemoryResolver},
        Document,
    },
    blueprint_memory::{self as model, CuratorEvidence, Entry},
};
use rusqlite::{params, Connection};
pub(crate) mod calls;
mod candidates;
pub(crate) mod cleanup;
mod curator;
pub(crate) mod index;
mod library;
pub(crate) mod metadata;
mod search;
mod show;

pub(crate) fn initialize(conn: &Connection) -> Result<(), String> {
    index::initialize(conn)?;
    calls::initialize(conn)?;
    conn.execute_batch(cleanup::SCHEMA)
        .map_err(|e| e.to_string())
}

pub(crate) fn vector_projection(
    conn: &Connection,
    id: &str,
) -> Result<Option<(IndexRow, Vec<String>)>, String> {
    let Some(row) = index::read_row(conn, id)? else {
        return Ok(None);
    };
    let intents = metadata::read(conn, id)?
        .and_then(|record| record.blueprint)
        .map(|stats| stats.intents)
        .unwrap_or_default();
    Ok(Some((row, intents)))
}

impl BlueprintStore for SqliteMemStore {
    fn blueprint_save(
        &self,
        ctx: &WriteContext,
        area: &str,
        prepared: PreparedBlueprint,
        merge: bool,
        area_cap: usize,
        purpose: Option<Purpose>,
        embedder: &ActiveEmbedder,
    ) -> Result<SaveResult, String> {
        self.save_blueprint(ctx, area, prepared, merge, area_cap, purpose, embedder)
    }
    fn blueprint_entries(
        &self,
        ctx: &WriteContext,
        area: &str,
        validate: bool,
        team: Option<&str>,
    ) -> Result<Vec<Entry>, String> {
        library::load_entries_scoped(
            ctx,
            &self.conn.lock(),
            &self.blobs_dir,
            area,
            validate,
            team,
        )
    }
    fn blueprint_candidates(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: Option<&str>,
        identity: Option<&EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String> {
        self.candidates(ctx, area, team, identity)
    }
    fn blueprint_readonly(&self, area: &str) -> Result<Vec<Entry>, String> {
        self.blueprint_solutions_readonly(area)
    }
    fn blueprint_archive(&self, ctx: &WriteContext, area: &str, id: &str) -> Result<(), String> {
        self.archive_blueprint_solution(ctx, area, id)
    }
    fn blueprint_reuse(&self, ctx: &WriteContext, ids: &[String]) -> Result<(), String> {
        self.reuse_blueprints(ctx, ids)
    }
    fn blueprint_dream(
        &self,
        ctx: &WriteContext,
        embedder: &ActiveEmbedder,
        turn: u64,
    ) -> Result<BlueprintChanges, String> {
        self.dream_blueprints(ctx, embedder, turn)
    }
    fn blueprint_solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String> {
        self.solution_show(area, id)
    }
    fn blueprint_pin_calls(
        &self,
        _ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Document, String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let resolver = calls::ScopedResolver {
            conn: &tx,
            blobs: &self.blobs_dir,
            area,
            team,
        };
        let document = crate::blueprint_library::call_revisions::freeze(Some(&resolver), document)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(document)
    }
    fn blueprint_pin_bundle(
        &self,
        _ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
        local: &[LocalCallee],
    ) -> Result<Document, String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let resolver = calls::ScopedResolver {
            conn: &tx,
            blobs: &self.blobs_dir,
            area,
            team,
        };
        let document =
            crate::blueprint_library::call_revisions::freeze_bundle(document, local, &resolver)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(document)
    }
    fn blueprint_call_head(&self, area: &str, team: &str, id: &str) -> Result<Document, String> {
        calls::ScopedResolver {
            conn: &self.conn.lock(),
            blobs: &self.blobs_dir,
            area,
            team,
        }
        .head(id)
    }
    fn blueprint_call_revision(
        &self,
        area: &str,
        team: &str,
        pin: &CallPin,
    ) -> Result<Document, String> {
        calls::ScopedResolver {
            conn: &self.conn.lock(),
            blobs: &self.blobs_dir,
            area,
            team,
        }
        .revision(pin)
    }
    fn blueprint_call_head_for_drift(
        &self,
        area: &str,
        team: &str,
        id: &str,
    ) -> Result<(Document, Option<String>), String> {
        calls::ScopedResolver {
            conn: &self.conn.lock(),
            blobs: &self.blobs_dir,
            area,
            team,
        }
        .head_for_drift(id)
    }
    fn blueprint_call_drift(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Vec<Drift>, String> {
        let closure =
            codeterm_blueprint_types::blueprint_document::calls::FrozenClosure::from_document(
                document,
            )?;
        let conn = self.conn.lock();
        crate::blueprint_library::call_revisions::drift(
            &closure,
            &calls::ScopedResolver {
                conn: &conn,
                blobs: &self.blobs_dir,
                area,
                team,
            },
        )
    }
    fn blueprint_index(&self, id: &str) -> Result<Option<IndexRow>, String> {
        index::read_row(&self.conn.lock(), id)
    }
    fn blueprint_index_snapshot(&self, id: &str) -> Result<(IndexRow, String), String> {
        let conn = self.conn.lock();
        let row = index::read_row(&conn, id)?.ok_or("Blueprint index row absent")?;
        let body = store::get(&conn, id, &self.blobs_dir)?
            .ok_or("Blueprint solution disappeared")?
            .body;
        Ok((row, body))
    }
    fn blueprint_complete_enrichment(
        &self,
        _ctx: &WriteContext,
        id: &str,
        document_hash: &str,
        enrichment: Enrichment,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        let Some(mut row) = index::read_row(&conn, id)? else {
            return Ok(false);
        };
        if row.document_hash != document_hash {
            return Ok(false);
        }
        row.enrichment = Some(enrichment);
        index::write_row(&conn, id, &row)?;
        Ok(true)
    }
    fn blueprint_indexed_items(&self) -> Result<Vec<String>, String> {
        index::indexed_items(&self.conn.lock())
    }
    fn blueprint_vector_projection(
        &self,
        id: &str,
    ) -> Result<Option<(IndexRow, Vec<String>)>, String> {
        vector_projection(&self.conn.lock(), id)
    }
    fn blueprint_vectors(&self, id: &str, identity: &EmbedderIdentity) -> Result<Vectors, String> {
        index::read_vectors(&self.conn.lock(), id, identity)
    }
    fn blueprint_pair(
        &self,
        a: &str,
        b: &str,
        embedder: &ActiveEmbedder,
    ) -> Result<(IndexedVectors, IndexedVectors), String> {
        let conn = self.conn.lock();
        let row = |id: &str| -> Result<_, String> {
            Ok((
                index::read_row(&conn, id)?.ok_or_else(|| format!("{id} is not indexed"))?,
                index::read_vectors(&conn, id, &embedder.identity())?,
            ))
        };
        Ok((row(a)?, row(b)?))
    }
    fn blueprint_replace_vectors(
        &self,
        _ctx: &WriteContext,
        id: &str,
        embedder: &ActiveEmbedder,
        identity: &EmbedderIdentity,
        keep: &[VectorKey],
        written: &[(VectorKey, String, Vec<f32>)],
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        if embedder.identity() != *identity {
            return Err("Blueprint embedder changed during indexing".into());
        }
        index::replace_vectors(&conn, id, embedder, identity, keep, written)
    }
    fn solution_candidates(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
        embedding: Option<&StampedVector>,
    ) -> Result<SolutionCandidates, String> {
        self.solution_candidates(query, source, embedding)
    }
    fn solution_metadata(
        &self,
        id: &str,
    ) -> Result<Option<crate::solution_metadata::SolutionMetadata>, String> {
        metadata::read(&self.conn.lock(), id)
    }
    fn curator_skill_save(
        &self,
        ctx: &WriteContext,
        item: NewMemItem,
        improve: Option<&str>,
        proof: CuratorEvidence,
        now: i64,
    ) -> Result<String, String> {
        self.save_curator_skill(ctx, item, improve, proof, now)
    }
}
