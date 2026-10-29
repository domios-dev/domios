use super::{
    catalog,
    index::{self, IndexRow, Slot, Vectors},
    recall::{self, Candidate},
    BlueprintHit, RecallConfig,
};
use crate::{embedder::EmbedderIdentity, MemManager};
use codeterm_blueprint_types::blueprint_memory::Entry;
use serde::Serialize;

pub const MAX_CARDS: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct RecallState {
    pub status: &'static str,
    pub reason: Option<String>,
    pub embedder: EmbedderIdentity,
    pub vectors_pending: usize,
    pub enrichment_pending: usize,
    pub reembed: crate::embedder::ReembedProgress,
}

pub struct AuthoringRecall {
    pub hits: Vec<BlueprintHit>,
    pub state: RecallState,
    pub unavailable: Vec<(String, String)>,
}

#[derive(Default)]
pub(super) struct CatalogCache {
    identity: Option<EmbedderIdentity>,
    candidates: Vec<Candidate>,
}

impl MemManager {
    pub fn recall_authoring_blueprints(
        &self,
        area: &str,
        team: &str,
        intent: &str,
        limit: usize,
    ) -> Result<AuthoringRecall, String> {
        if area.is_empty() || team.is_empty() {
            return Err("Typed call recall requires a Team and workspace scope".into());
        }
        if intent.trim().is_empty() || intent.len() > 4096 || intent.contains('\0') {
            return Err("Intent must contain 1..4096 UTF-8 bytes without NUL".into());
        }
        if !(1..=MAX_CARDS).contains(&limit) {
            return Err("Recall limit must be between 1 and 20".into());
        }
        let identity = self.embedder.identity();
        let mut state = RecallState {
            status: "ready",
            reason: None,
            embedder: identity.clone(),
            vectors_pending: 0,
            enrichment_pending: 0,
            reembed: self.reembed.snapshot(),
        };
        let mut unavailable = catalog::UNAVAILABLE
            .iter()
            .map(|(id, reason)| ((*id).into(), (*reason).into()))
            .collect::<Vec<_>>();
        let query = self.embedder.query(intent).filter(|v| {
            v.identity == identity && index::valid_dimension(&v.vector, identity.dimension)
        });
        let Some(query) = query else {
            state.status = "degraded";
            state.reason = Some("Current query embeddings unavailable; vector recall required, no lexical substitute".into());
            return Ok(AuthoringRecall {
                hits: vec![],
                state,
                unavailable,
            });
        };
        let mut candidates = self.scoped_candidates(area, team, &identity)?;
        candidates.extend(self.catalog_candidates(&identity, &mut unavailable)?);
        state.vectors_pending = unavailable
            .iter()
            .filter(|(_, reason)| reason == "Catalog embedding pending")
            .count();
        let mut eligible = Vec::new();
        for mut candidate in candidates {
            retain_current_vectors(&mut candidate);
            let doc = &candidate.entry.document;
            if doc.trigger.is_some() || doc.result.port.is_none() || doc.result.from.is_none() {
                unavailable.push((
                    candidate.entry.id.clone(),
                    "Saved graph has no native typed callable signature".into(),
                ));
                continue;
            }
            if candidate
                .row
                .as_ref()
                .is_none_or(IndexRow::enrichment_pending)
            {
                state.enrichment_pending += 1;
            }
            if candidate.vectors.slot(Slot::Intent).next().is_none() {
                state.vectors_pending += 1;
                continue;
            }
            if candidate.vectors.slot(Slot::Canon).next().is_none() {
                state.vectors_pending += 1;
            }
            eligible.push(candidate);
        }
        if self.embedder.identity() != identity {
            state.status = "degraded";
            state.reason =
                Some("Embedder changed during recall; no mixed-model cards returned".into());
            return Ok(AuthoringRecall {
                hits: vec![],
                state,
                unavailable,
            });
        }
        if state.vectors_pending > 0 {
            state.status = "degraded";
            state.reason = Some("Current vectors pending; candidates without mandatory intent vectors omitted, no lexical substitute".into());
        }
        let mut hits = recall::rank(
            eligible,
            intent,
            Some(&query.vector),
            None,
            &RecallConfig::default(),
            chrono::Utc::now().timestamp(),
        );
        hits.truncate(limit);
        Ok(AuthoringRecall {
            hits,
            state,
            unavailable,
        })
    }

    fn scoped_candidates(
        &self,
        area: &str,
        team: &str,
        identity: &EmbedderIdentity,
    ) -> Result<Vec<Candidate>, String> {
        self.store.blueprint_candidates(
            &Self::blueprint_write_context(),
            area,
            Some(team),
            Some(identity),
        )
    }

    fn catalog_candidates(
        &self,
        identity: &EmbedderIdentity,
        unavailable: &mut Vec<(String, String)>,
    ) -> Result<Vec<Candidate>, String> {
        let mut cache = self.blueprint.catalog.lock();
        if cache.identity.as_ref() != Some(identity) {
            cache.candidates.clear();
            cache.identity = Some(identity.clone());
        }
        for item in catalog::ENTRIES {
            if cache
                .candidates
                .iter()
                .any(|candidate| candidate.entry.id == item.id)
            {
                continue;
            }
            let doc = match catalog::document(item.id) {
                Ok(doc) => doc,
                Err(error) => {
                    unavailable.push((item.id.into(), error));
                    continue;
                }
            };
            let diagnostics = codeterm_blueprint_types::blueprint_document::checker::check_document(
                &doc,
                &Default::default(),
            );
            if codeterm_blueprint_types::blueprint_document::checker::has_errors(&diagnostics) {
                unavailable.push((
                    item.id.into(),
                    "Catalog body fails the native callable checker".into(),
                ));
                continue;
            }
            let intents = vec![doc.intent.clone().unwrap_or_else(|| doc.name.clone())];
            let row = super::qualification::index_row(&doc, &intents);
            let mut vectors = Vectors::default();
            for (slot, text) in [(Slot::Intent, &intents[0]), (Slot::Canon, &row.canon)] {
                if let Ok(vector) = self.embedder.passage(text) {
                    if vector.identity == *identity
                        && index::valid_dimension(&vector.vector, identity.dimension)
                    {
                        vectors
                            .by_slot
                            .insert((slot, 0), (text.clone(), vector.vector));
                    }
                }
            }
            let candidate = Candidate {
                entry: Entry {
                    id: item.id.into(),
                    document: doc,
                    intents,
                    uses: 0,
                    successes: 0,
                    failures: 0,
                    kept: false,
                    last_used: 0,
                },
                row: Some(row),
                vectors,
            };
            if candidate.vectors.slot(Slot::Intent).next().is_some()
                && candidate.vectors.slot(Slot::Canon).next().is_some()
            {
                cache.candidates.push(candidate);
            } else {
                // Incomplete vectors are retried by the next request, never treated as lexical recall.
                unavailable.push((item.id.into(), "Catalog embedding pending".into()));
            }
        }
        Ok(cache.candidates.clone())
    }
}

fn retain_current_vectors(candidate: &mut Candidate) {
    candidate
        .vectors
        .by_slot
        .retain(|(slot, _), (text, _)| match slot {
            Slot::Intent => candidate.entry.intents.contains(text),
            Slot::Canon => candidate.row.as_ref().is_some_and(|row| row.canon == *text),
            Slot::Summary => candidate
                .row
                .as_ref()
                .and_then(IndexRow::current_enrichment)
                .is_some_and(|enrichment| enrichment.summary == *text),
            Slot::WorkerIntent => candidate
                .row
                .as_ref()
                .and_then(IndexRow::current_enrichment)
                .is_some_and(|enrichment| enrichment.intents.contains(text)),
        });
}

#[cfg(test)]
mod tests;
