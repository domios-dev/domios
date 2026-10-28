use crate::{db_error, items, PostgresMemStore};
use codeterm_mem::blueprint_library::SOURCE;
mod calls;
mod curator;
mod index;
mod library;
mod metadata;
mod search;
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
        ctx: &WriteContext,
        area: &str,
        prepared: PreparedBlueprint,
        merge: bool,
        area_cap: usize,
        purpose: Option<Purpose>,
        embedder: &ActiveEmbedder,
    ) -> Result<SaveResult, String> {
        self.transaction(|tx| {
            library::save(tx, ctx, area, prepared, merge, area_cap, purpose, embedder)
        })
    }
    fn blueprint_entries(
        &self,
        ctx: &WriteContext,
        area: &str,
        validate: bool,
        team: Option<&str>,
    ) -> Result<Vec<Entry>, String> {
        self.transaction(|tx| library::load_entries(tx, ctx, area, validate, team))
    }
    fn blueprint_candidates(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: Option<&str>,
        identity: Option<&EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String> {
        self.transaction(|tx| {
            library::load_entries(tx, ctx, area, true, team)?
                .into_iter()
                .map(|entry| {
                    Ok(Candidate {
                        row: index::read_row(tx, &entry.id)?,
                        vectors: identity
                            .map(|identity| index::read_vectors(tx, &entry.id, identity))
                            .transpose()?
                            .unwrap_or_default(),
                        entry,
                    })
                })
                .collect()
        })
    }
    fn blueprint_readonly(&self, area: &str) -> Result<Vec<Entry>, String> {
        self.transaction(|tx| library::readonly(tx, area))
    }
    fn blueprint_archive(&self, ctx: &WriteContext, area: &str, id: &str) -> Result<(), String> {
        self.transaction(|tx| {
            if !library::load_entries(tx, ctx, area, true, None)?
                .iter()
                .any(|entry| entry.id == id)
            {
                return Err("Saved Blueprint recipe not found in this project".into());
            }
            library::archive(tx, ctx, id, "Archived by owner")
        })
    }
    fn blueprint_reuse(&self, ctx: &WriteContext, ids: &[String]) -> Result<(), String> {
        self.transaction(|tx| library::reuse(tx, ctx, ids))
    }
    fn blueprint_dream(
        &self,
        ctx: &WriteContext,
        embedder: &ActiveEmbedder,
        turn: u64,
    ) -> Result<BlueprintChanges, String> {
        self.transaction(|tx| library::dream(tx, ctx, embedder, turn))
    }
    fn blueprint_solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String> {
        self.transaction(|tx| {
            if area.is_empty() {
                return Err("Saved solution requires a workspace".into());
            }
            let id = id.strip_prefix("mem:").unwrap_or(id);
            let sql = "SELECT c.team FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id LEFT JOIN blueprint_call_scopes c ON c.item_id=m.id AND c.area=m.area WHERE m.id=$1 AND m.area=$2 AND m.kind='skill' AND m.source=$3";
            let row = tx.query_opt(sql, &[&id, &area, &SOURCE])
                .map_err(db_error)?
                .ok_or("Saved Blueprint solution not found in this workspace")?;
            let team: Option<String> = row.get(0);
            let item = items::get_in(tx, id)?.ok_or("Saved Blueprint solution disappeared")?;
            let resolver = team.as_deref().map(|team| calls::ScopedResolver::new(tx, area, team));
            codeterm_mem::blueprint_library::show::project(
                codeterm_blueprint_types::blueprint_memory::parse_document(&item.body)?,
                |pin| {
                    use codeterm_blueprint_types::blueprint_document::calls::MemoryResolver;
                    resolver.as_ref().ok_or("Saved call requires a Team")?.revision(pin)
                },
            )
        })
    }
    fn blueprint_pin_calls(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Document, String> {
        self.transaction(|tx| {
            let _ = ctx;
            codeterm_mem::blueprint_library::call_revisions::freeze(
                Some(&calls::ScopedResolver::new(tx, area, team)),
                document,
            )
        })
    }
    fn blueprint_pin_bundle(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: &str,
        document: Document,
        local: &[LocalCallee],
    ) -> Result<Document, String> {
        self.transaction(|tx| {
            let _ = ctx;
            codeterm_mem::blueprint_library::call_revisions::freeze_bundle(
                document,
                local,
                &calls::ScopedResolver::new(tx, area, team),
            )
        })
    }
    fn blueprint_call_head(&self, area: &str, team: &str, id: &str) -> Result<Document, String> {
        self.transaction(|tx| {
            use codeterm_blueprint_types::blueprint_document::calls::MemoryResolver;
            calls::ScopedResolver::new(tx, area, team).head(id)
        })
    }
    fn blueprint_call_revision(
        &self,
        area: &str,
        team: &str,
        pin: &CallPin,
    ) -> Result<Document, String> {
        self.transaction(|tx| {
            use codeterm_blueprint_types::blueprint_document::calls::MemoryResolver;
            calls::ScopedResolver::new(tx, area, team).revision(pin)
        })
    }
    fn blueprint_call_head_for_drift(
        &self,
        area: &str,
        team: &str,
        id: &str,
    ) -> Result<(Document, Option<String>), String> {
        self.transaction(|tx| {
            use codeterm_blueprint_types::blueprint_document::calls::MemoryResolver;
            calls::ScopedResolver::new(tx, area, team).head_for_drift(id)
        })
    }
    fn blueprint_call_drift(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Vec<Drift>, String> {
        self.transaction(|tx| {
            let closure =
                codeterm_blueprint_types::blueprint_document::calls::FrozenClosure::from_document(
                    document,
                )?;
            codeterm_mem::blueprint_library::call_revisions::drift(
                &closure,
                &calls::ScopedResolver::new(tx, area, team),
            )
        })
    }
    fn blueprint_index(&self, id: &str) -> Result<Option<IndexRow>, String> {
        self.transaction(|tx| index::read_row(tx, id))
    }
    fn blueprint_index_snapshot(&self, id: &str) -> Result<(IndexRow, String), String> {
        self.transaction(|tx| {
            Ok((
                index::read_row(tx, id)?.ok_or("Blueprint index row absent")?,
                items::get_in(tx, id)?
                    .ok_or("Blueprint solution disappeared")?
                    .body,
            ))
        })
    }
    fn blueprint_complete_enrichment(
        &self,
        ctx: &WriteContext,
        id: &str,
        document_hash: &str,
        enrichment: Enrichment,
    ) -> Result<bool, String> {
        self.transaction(|tx| {
            let _ = ctx;
            let Some(mut row) = index::read_row(tx, id)? else {
                return Ok(false);
            };
            if row.document_hash != document_hash {
                return Ok(false);
            }
            row.enrichment = Some(enrichment);
            index::write_row(tx, id, &row)?;
            Ok(true)
        })
    }
    fn blueprint_indexed_items(&self) -> Result<Vec<String>, String> {
        self.transaction(|tx| {
            let sql = "SELECT b.item_id FROM blueprint_index b JOIN mem_items m ON m.id=b.item_id WHERE m.kind='skill' AND m.source=$1 ORDER BY b.item_id";
            Ok(tx.query(sql, &[&SOURCE]).map_err(db_error)?
                .into_iter().map(|row| row.get(0)).collect())
        })
    }
    fn blueprint_vector_projection(
        &self,
        id: &str,
    ) -> Result<Option<(IndexRow, Vec<String>)>, String> {
        self.transaction(|tx| {
            let Some(row) = index::read_row(tx, id)? else {
                return Ok(None);
            };
            let intents = metadata::read(tx, id)?
                .and_then(|record| record.blueprint)
                .map(|stats| stats.intents)
                .unwrap_or_default();
            Ok(Some((row, intents)))
        })
    }
    fn blueprint_vectors(&self, id: &str, identity: &EmbedderIdentity) -> Result<Vectors, String> {
        self.transaction(|tx| index::read_vectors(tx, id, identity))
    }
    fn blueprint_pair(
        &self,
        a: &str,
        b: &str,
        embedder: &ActiveEmbedder,
    ) -> Result<(IndexedVectors, IndexedVectors), String> {
        self.transaction(|tx| {
            let identity = embedder.identity();
            let mut read = |id: &str| -> Result<_, String> {
                Ok((
                    index::read_row(tx, id)?.ok_or_else(|| format!("{id} is not indexed"))?,
                    index::read_vectors(tx, id, &identity)?,
                ))
            };
            Ok((read(a)?, read(b)?))
        })
    }
    fn blueprint_replace_vectors(
        &self,
        ctx: &WriteContext,
        id: &str,
        embedder: &ActiveEmbedder,
        identity: &EmbedderIdentity,
        keep: &[VectorKey],
        written: &[(VectorKey, String, Vec<f32>)],
    ) -> Result<(), String> {
        self.transaction(|tx| {
            if embedder.identity() != *identity {
                return Err("Blueprint embedder changed during indexing".into());
            }
            index::replace_vectors(tx, ctx, id, embedder, identity, keep, written)
        })
    }
    fn solution_candidates(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
        embedding: Option<&StampedVector>,
    ) -> Result<SolutionCandidates, String> {
        self.transaction(|tx| search::candidates(tx, query, source, embedding))
    }
    fn solution_metadata(&self, id: &str) -> Result<Option<SolutionMetadata>, String> {
        self.transaction(|tx| metadata::read(tx, id))
    }
    fn curator_skill_save(
        &self,
        ctx: &WriteContext,
        item: NewMemItem,
        improve: Option<&str>,
        proof: CuratorEvidence,
        now: i64,
    ) -> Result<String, String> {
        self.transaction(|tx| curator::save(tx, ctx, item, improve, proof, now))
    }
}
