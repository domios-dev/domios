use super::index::{self, Enrichment, Provenance, Slot, VectorKey};
use super::services::{BlueprintEnricher, EnrichmentJob};
use crate::MemManager;
use codeterm_blueprint_types::blueprint_memory::{contains_credentials, validate_intents};
use std::sync::Arc;

const SUMMARY_CHARS: usize = 800;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnrichmentStatus {
    Ready,
    Requested,
    Pending(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexReport {
    pub vectors: usize,
    pub missing_vectors: usize,
    pub enrichment: EnrichmentStatus,
}

pub fn enrichment_task(document_yaml: &str, gists: &str) -> String {
    crate::prompt_text::BLUEPRINT_ENRICHMENT
        .replace("{document}", document_yaml.trim_end())
        .replace("{gists}", gists)
}

pub fn parse_enrichment_report(report: &str) -> Result<(String, Vec<String>), String> {
    let line = report
        .lines()
        .rev()
        .find(|line| line.contains("WHAT:"))
        .ok_or("Enrichment report has no WHAT: line")?;
    let mut summary = None;
    let mut intents = Vec::new();
    for part in line.split("||") {
        let part = part
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if let Some(text) = part.strip_prefix("WHAT:") {
            summary = Some(text.trim().to_owned());
        } else if let Some(text) = part.strip_prefix("INTENT:") {
            intents.push(text.trim().to_owned());
        }
    }
    let summary = summary
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| {
            !text.is_empty()
                && text.chars().count() <= SUMMARY_CHARS
                && !text.chars().any(char::is_control)
        })
        .ok_or("Enrichment summary must contain 1..800 plain-text characters")?;
    let intents = validate_intents(&intents)?;
    if contains_credentials(&summary) || intents.iter().any(|intent| contains_credentials(intent)) {
        return Err("Enrichment report contains credential-like text".into());
    }
    Ok((summary, intents))
}

pub(crate) struct Desired {
    pub(crate) texts: Vec<(VectorKey, String)>,
}

impl MemManager {
    #[cfg(test)]
    pub(crate) fn set_blueprint_embedder(
        &self,
        provider: crate::embedder::provider::SharedProvider,
    ) {
        self.embedder.replace(provider);
    }

    pub fn set_blueprint_enricher(&self, enricher: Option<Arc<dyn BlueprintEnricher>>) {
        *self.blueprint.enricher.write() = enricher;
    }

    pub fn queue_blueprint_backfill(self: &Arc<Self>) -> Result<(), String> {
        self.queue_blueprint(super::Pending::Backfill)
    }

    pub fn blueprint_index(&self, id: &str) -> Result<Option<index::IndexRow>, String> {
        self.store.blueprint_index(id)
    }

    pub fn index_blueprint(&self, id: &str) -> Result<IndexReport, String> {
        let vectors = self.index_blueprint_vectors(id)?;
        let (row, body) = self.store.blueprint_index_snapshot(id)?;
        let enrichment = if !row.enrichment_pending() {
            EnrichmentStatus::Ready
        } else {
            match self.blueprint.enricher() {
                None => EnrichmentStatus::Pending("no enrichment worker".into()),
                Some(enricher) => match enricher.request(EnrichmentJob {
                    item_id: id.into(),
                    document_hash: row.document_hash.clone(),
                    task: enrichment_task(&body, &row.gists),
                }) {
                    Ok(()) => EnrichmentStatus::Requested,
                    Err(error) => EnrichmentStatus::Pending(error),
                },
            }
        };
        if let EnrichmentStatus::Pending(reason) = &enrichment {
            log::debug!("[blueprint-memory] enrichment pending for {id}: {reason}");
        }
        Ok(IndexReport {
            vectors: vectors.0,
            missing_vectors: vectors.1,
            enrichment,
        })
    }

    pub fn complete_blueprint_enrichment(
        &self,
        id: &str,
        document_hash: &str,
        model: &str,
        report: &str,
    ) -> Result<bool, String> {
        let (summary, intents) = parse_enrichment_report(report)?;
        let enrichment = Enrichment {
            summary,
            intents,
            provenance: Provenance {
                model: model.into(),
                document_hash: document_hash.into(),
                created_at: chrono::Utc::now().timestamp(),
            },
        };
        if !self.store.blueprint_complete_enrichment(
            &Self::blueprint_write_context(),
            id,
            document_hash,
            enrichment,
        )? {
            return Ok(false);
        }
        self.index_blueprint_vectors(id)?;
        Ok(true)
    }

    pub fn backfill_blueprints(&self) -> Result<Vec<(String, IndexReport)>, String> {
        let ids = self.store.blueprint_indexed_items()?;
        let mut reports = Vec::new();
        for id in ids {
            match self.index_blueprint(&id) {
                Ok(report) => reports.push((id, report)),
                Err(error) => log::warn!("[blueprint-memory] backfill {id}: {error}"),
            }
        }
        Ok(reports)
    }

    fn desired_vectors(&self, id: &str) -> Result<Option<Desired>, String> {
        let Some((row, intents)) = self.store.blueprint_vector_projection(id)? else {
            return Ok(None);
        };
        Ok(Some(desired_vectors(&row, &intents)))
    }
}

pub(crate) fn desired_vectors(row: &index::IndexRow, intents: &[String]) -> Desired {
    let mut texts = intents
        .iter()
        .cloned()
        .enumerate()
        .map(|(ordinal, text)| ((Slot::Intent, ordinal), text))
        .collect::<Vec<_>>();
    texts.push(((Slot::Canon, 0), row.canon.clone()));
    if let Some(enrichment) = row.current_enrichment() {
        texts.push(((Slot::Summary, 0), enrichment.summary.clone()));
        texts.extend(
            enrichment
                .intents
                .iter()
                .enumerate()
                .map(|(ordinal, text)| ((Slot::WorkerIntent, ordinal), text.clone())),
        );
    }
    Desired { texts }
}

pub(crate) fn blueprint_vector_texts(
    row: &index::IndexRow,
    intents: &[String],
) -> Vec<(String, usize, String)> {
    desired_vectors(row, intents)
        .texts
        .into_iter()
        .map(|((slot, ordinal), text)| (slot.name().into(), ordinal, text))
        .collect()
}

impl MemManager {
    fn index_blueprint_vectors(&self, id: &str) -> Result<(usize, usize), String> {
        let Some(desired) = self.desired_vectors(id)? else {
            return Ok((0, 0));
        };
        let identity = self.embedder.identity();
        let existing = self.store.blueprint_vectors(id, &identity)?;
        let mut written = Vec::new();
        let mut keep = Vec::new();
        let mut missing = 0;
        for (key, text) in &desired.texts {
            if existing
                .by_slot
                .get(key)
                .is_some_and(|(stored, _)| stored == text)
            {
                keep.push(*key);
                continue;
            }
            match self.embedder.passage(text) {
                Ok(vector)
                    if vector.identity == identity
                        && index::valid_dimension(&vector.vector, identity.dimension) =>
                {
                    keep.push(*key);
                    written.push((*key, text.clone(), vector.vector));
                }
                Ok(_) => missing += 1,
                Err(error) => {
                    log::debug!("[blueprint-memory] vectors pending for {id}: {error}");
                    missing += 1;
                }
            }
        }
        if self.embedder.identity() != identity {
            return Err("Blueprint embedder changed during indexing".into());
        }
        self.store.blueprint_replace_vectors(
            &Self::blueprint_write_context(),
            id,
            &self.embedder,
            &identity,
            &keep,
            &written,
        )?;
        Ok((keep.len(), missing))
    }
}

#[cfg(test)]
mod tests;
