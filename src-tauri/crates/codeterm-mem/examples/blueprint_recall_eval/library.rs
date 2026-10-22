use crate::fixtures::{CachedEnrichment, CachedIntents, CorpusDoc, SaveInputs};
use codeterm_blueprint_types::blueprint_memory::{parse_document, Evidence};
use codeterm_mem::blueprint_library::{
    enrichment::EnrichmentStatus, BlueprintEnricher, EnrichmentJob,
};
use codeterm_mem::MemManager;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const ALL_AREAS: &str = "";
const AREA_PREFIX: &str = "blueprints:eval-";
const PER_AREA: usize = 35;
pub const WORKER_MODEL: &str = "claude-haiku-4-5-20251001";

#[derive(Default)]
pub struct CapturingEnricher {
    pub jobs: Mutex<Vec<EnrichmentJob>>,
}

impl BlueprintEnricher for CapturingEnricher {
    fn request(&self, job: EnrichmentJob) -> Result<(), String> {
        self.jobs.lock().push(job);
        Ok(())
    }
}

pub struct Library {
    pub mem: Arc<MemManager>,
    pub doc_of: BTreeMap<String, String>,
    pub item_of: BTreeMap<String, String>,
    pub jobs: BTreeMap<String, EnrichmentJob>,
    pub enriched: usize,
    pub refused: BTreeMap<String, String>,
    _dir: tempfile::TempDir,
}

pub enum Intents<'a> {
    Placeholder,
    Cached(&'a SaveInputs<CachedIntents>),
}

pub fn ingest(
    corpus: &[CorpusDoc],
    intents: Intents<'_>,
    enrichment: Option<&SaveInputs<CachedEnrichment>>,
) -> Result<Library, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mem = MemManager::open_offline(dir.path())?;
    let enricher = Arc::new(CapturingEnricher::default());
    mem.set_blueprint_enricher(Some(enricher.clone()));
    let mut doc_of = BTreeMap::new();
    let mut item_of = BTreeMap::new();
    let mut refused = BTreeMap::new();
    let mut per_area: BTreeMap<String, usize> = BTreeMap::new();
    for (position, doc) in corpus.iter().enumerate() {
        let area = format!("{AREA_PREFIX}{}", position / PER_AREA);
        let document =
            parse_document(&doc.source).map_err(|error| format!("{}: {error}", doc.id))?;
        let doc_intents = match &intents {
            Intents::Placeholder => vec![document.name.clone()],
            Intents::Cached(cache) => match cache.docs.get(&doc.id) {
                Some(cached) => cached.intents.clone(),
                None => {
                    refused.insert(doc.id.clone(), "no cached agent intents".into());
                    continue;
                }
            },
        };
        let evidence = Evidence {
            kept: true,
            successes: 1,
            runs: 1,
            recognized: 1,
            name: document.name.clone(),
            note: document.note.clone().unwrap_or_default(),
            ..Evidence::default()
        };
        let id = match mem.save_blueprint_without_merge(&area, document, &doc_intents, evidence) {
            Ok(saved) => saved.id,
            Err(error) => {
                refused.insert(doc.id.clone(), error);
                continue;
            }
        };
        let report = mem.index_blueprint(&id)?;
        if !matches!(intents, Intents::Placeholder) && report.missing_vectors > 0 {
            return Err(format!(
                "{}: {} vectors missing; the global embedder is unavailable",
                doc.id, report.missing_vectors
            ));
        }
        if let Intents::Cached(cache) = &intents {
            let hash = &mem.blueprint_index(&id)?.ok_or("index row")?.document_hash;
            if cache.docs[&doc.id].document_hash != *hash {
                return Err(format!(
                    "cached intents for {} were written for another document hash",
                    doc.id
                ));
            }
        }
        *per_area.entry(area).or_default() += 1;
        doc_of.insert(id.clone(), doc.id.clone());
        item_of.insert(doc.id.clone(), id);
    }
    for (area, saved) in &per_area {
        let live = mem.list_blueprint_solutions(area)?.len();
        if live != *saved {
            return Err(format!("{area}: {live} of {saved} saved documents are recallable; the area cap evicted the rest"));
        }
    }
    let jobs = enricher
        .jobs
        .lock()
        .drain(..)
        .map(|job| (doc_of[&job.item_id].clone(), job))
        .collect::<BTreeMap<_, _>>();
    let mut enriched = 0;
    if let Some(cache) = enrichment {
        for (doc, job) in &jobs {
            let Some(cached) = cache.docs.get(doc) else {
                continue;
            };
            if cached.document_hash != job.document_hash {
                return Err(format!(
                    "cached enrichment for {doc} was written for another document hash"
                ));
            }
            match mem.complete_blueprint_enrichment(
                &job.item_id,
                &job.document_hash,
                &cache.model,
                &cached.report,
            ) {
                Ok(true) => enriched += 1,
                Ok(false) => return Err(format!("{doc}: enrichment did not apply")),
                Err(error) => eprintln!("{doc}: enrichment refused ({error}); stays pending"),
            }
        }
        for (doc, item) in &item_of {
            let status = mem.index_blueprint(item)?.enrichment;
            if status != EnrichmentStatus::Ready && cache.docs.get(doc).is_some_and(|c| c.parsed) {
                return Err(format!("{doc}: parsed enrichment is not ready"));
            }
        }
        enricher.jobs.lock().clear();
    }
    Ok(Library {
        mem,
        doc_of,
        item_of,
        jobs,
        enriched,
        refused,
        _dir: dir,
    })
}
