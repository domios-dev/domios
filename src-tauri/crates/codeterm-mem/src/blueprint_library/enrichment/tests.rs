use super::super::fakes::{RecordingEnricher, WordEmbedder};
use super::super::recall::RecallConfig;
use super::*;
use codeterm_blueprint_types::blueprint_memory::{parse_document, Evidence};
use std::sync::atomic::Ordering;

const WATCH: &str = r#"schema_version: 2
kind: watch
name: ci-red
note: Tell me when CI turns red.
steps:
- id: source
  uses: probe
  with:
    command:
      lit: gh pr checks 42 --watch
  every:
    lit: 1m
  timeout:
    lit: 1h
- id: decide
  uses: ask
  with:
    question:
      lit: is-ci-red
  buckets:
  - red
  matches:
    red:
      lit: fail
  until: red
  routes:
    red: alert
- id: alert
  uses: notify
"#;
const WATCH_REFORMATTED: &str = r#"{"kind": "watch", "name": "ci-red", "note": "Tell me when CI turns red.", "schema_version": 2, "steps": [{"every": {"lit": "1m"}, "id": "source", "timeout": {"lit": "1h"}, "uses": "probe", "with": {"command": {"lit": "gh pr checks 42 --watch"}}}, {"buckets": ["red"], "id": "decide", "matches": {"red": {"lit": "fail"}}, "routes": {"red": "alert"}, "until": "red", "uses": "ask", "with": {"question": {"lit": "is-ci-red"}}}, {"id": "alert", "uses": "notify"}]}"#;
const WATCH_CHANGED: &str = r#"schema_version: 2
kind: watch
name: ci-red
note: Tell me when CI turns red.
steps:
- id: source
  uses: probe
  with:
    command:
      lit: gh run list --branch main
  every:
    lit: 1m
  timeout:
    lit: 1h
- id: decide
  uses: ask
  with:
    question:
      lit: is-ci-red
  buckets:
  - red
  matches:
    red:
      lit: fail
  until: red
  routes:
    red: alert
- id: alert
  uses: notify
"#;
const REPORT: &str = "WHAT: Polls pull request checks and pings the owner as soon as a check breaks, using zebracorn status words. || INTENT: alert me when my pr checks break || INTENT: ping me on a red build || INTENT: tell me once a pull request check fails";

fn evidence() -> Evidence {
    Evidence {
        kept: true,
        successes: 1,
        ..Evidence::default()
    }
}

fn intents() -> Vec<String> {
    vec!["Watch CI and report failing checks".into()]
}

fn save(mem: &MemManager, yaml: &str) -> String {
    mem.save_blueprint(
        "project",
        parse_document(yaml).unwrap(),
        &intents(),
        evidence(),
    )
    .unwrap()
    .id
}

#[test]
fn enrichment_follows_the_document_hash_and_records_provenance() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let enricher = Arc::new(RecordingEnricher::default());
    mem.set_blueprint_enricher(Some(enricher.clone()));
    let id = save(&mem, WATCH);
    let first = enricher.jobs.lock().clone();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].item_id, id);
    assert!(
        first[0].task.contains("input: command"),
        "{}",
        first[0].task
    );
    assert!(
        first[0].task.contains("runs gh pr checks {n} watch"),
        "{}",
        first[0].task
    );
    assert!(!first[0].task.contains("Watch CI and report failing checks"));
    assert!(mem
        .complete_blueprint_enrichment(&id, &first[0].document_hash, "fake-model", REPORT)
        .unwrap());
    let row = mem.blueprint_index(&id).unwrap().unwrap();
    let enrichment = row.current_enrichment().unwrap();
    assert_eq!(enrichment.provenance.model, "fake-model");
    assert_eq!(enrichment.provenance.document_hash, first[0].document_hash);
    assert!(enrichment.provenance.created_at > 0);
    assert_eq!(enrichment.intents.len(), 3);

    assert_eq!(save(&mem, WATCH_REFORMATTED), id);
    assert_eq!(
        enricher.jobs.lock().len(),
        1,
        "formatting-only edit regenerated"
    );
    assert!(!mem
        .blueprint_index(&id)
        .unwrap()
        .unwrap()
        .enrichment_pending());

    assert_eq!(save(&mem, WATCH_CHANGED), id);
    let jobs = enricher.jobs.lock().clone();
    assert_eq!(jobs.len(), 2, "graph change did not regenerate");
    assert_ne!(jobs[1].document_hash, jobs[0].document_hash);
    assert!(mem
        .blueprint_index(&id)
        .unwrap()
        .unwrap()
        .enrichment_pending());
    assert!(!mem
        .complete_blueprint_enrichment(&id, &jobs[0].document_hash, "fake-model", REPORT)
        .unwrap());
    assert!(mem
        .complete_blueprint_enrichment(&id, &jobs[1].document_hash, "fake-model", REPORT)
        .unwrap());
}

#[test]
fn enrichment_text_is_used_for_retrieval_only() {
    let mem = MemManager::open_in_memory().unwrap();
    let enricher = Arc::new(RecordingEnricher::default());
    mem.set_blueprint_enricher(Some(enricher.clone()));
    let id = save(&mem, WATCH);
    let hash = enricher.jobs.lock()[0].document_hash.clone();
    mem.complete_blueprint_enrichment(&id, &hash, "fake-model", REPORT)
        .unwrap();
    let item = mem.get(&id).unwrap().unwrap();
    for stored in [item.title, item.body, item.summary.unwrap_or_default()] {
        assert!(!stored.contains("zebracorn"), "{stored}");
    }
    let recipe = mem
        .blueprint_recipe("project", &id)
        .unwrap()
        .yaml()
        .unwrap();
    assert!(!recipe.contains("zebracorn"));
    let enriched = mem
        .recall_blueprint_hits("project", "zebracorn", None, 5, &RecallConfig::default())
        .unwrap();
    assert_eq!(enriched[0].entry.id, id);
    assert!(enriched[0].why.contains("enrichment by fake-model"));
    let plain = RecallConfig {
        enrichment: false,
        ..RecallConfig::default()
    };
    assert!(mem
        .recall_blueprint_hits("project", "zebracorn", None, 5, &plain)
        .unwrap()
        .is_empty());
}

#[test]
fn unavailable_workers_never_block_a_save_and_backfill_completes_it() {
    let mem = MemManager::open_in_memory().unwrap();
    let embedder = WordEmbedder::new(false);
    mem.set_blueprint_embedder(embedder.clone());
    let enricher = Arc::new(RecordingEnricher::default());
    enricher.offline.store(true, Ordering::SeqCst);
    mem.set_blueprint_enricher(Some(enricher.clone()));
    let id = save(&mem, WATCH);
    let row = mem.blueprint_index(&id).unwrap().unwrap();
    assert!(row.enrichment_pending());
    let degraded = mem
        .recall_blueprint_hits("project", "gh pr checks", None, 5, &RecallConfig::default())
        .unwrap();
    assert_eq!(degraded[0].entry.id, id);
    assert!(degraded[0].enrichment_pending);
    assert!(degraded[0].why.contains("enrichment pending"));
    let report = mem.index_blueprint(&id).unwrap();
    assert_eq!(report.missing_vectors, 2);
    assert_eq!(
        report.enrichment,
        EnrichmentStatus::Pending("mem workers unavailable".into())
    );

    embedder.available.store(true, Ordering::SeqCst);
    enricher.offline.store(false, Ordering::SeqCst);
    let backfilled = mem.backfill_blueprints().unwrap();
    assert_eq!(backfilled.len(), 1);
    assert_eq!(backfilled[0].1.missing_vectors, 0);
    assert_eq!(backfilled[0].1.vectors, 2);
    assert_eq!(backfilled[0].1.enrichment, EnrichmentStatus::Requested);
    let job = enricher.jobs.lock()[0].clone();
    assert!(mem
        .complete_blueprint_enrichment(&id, &job.document_hash, "fake-model", REPORT)
        .unwrap());
    assert!(!mem
        .blueprint_index(&id)
        .unwrap()
        .unwrap()
        .enrichment_pending());
    let done = mem.backfill_blueprints().unwrap();
    assert_eq!(done[0].1.enrichment, EnrichmentStatus::Ready);
    assert_eq!(done[0].1.vectors, 6);
    assert_eq!(enricher.jobs.lock().len(), 1);
}

#[test]
fn enrichment_report_parsing_refuses_malformed_or_credential_text() {
    let (summary, intents) = parse_enrichment_report(&format!("noise\n'{REPORT}'\n")).unwrap();
    assert!(summary.starts_with("Polls pull request checks"));
    assert_eq!(intents.len(), 3);
    assert!(parse_enrichment_report("INTENT: only intents").is_err());
    assert!(parse_enrichment_report(
        "WHAT: fine || INTENT: a || INTENT: b || INTENT: c || INTENT: d || INTENT: e || INTENT: f"
    )
    .is_err());
    assert!(parse_enrichment_report(
        "WHAT: uses ghp_abcdefghijklmnopqrstuvwxyz0123456789 || INTENT: watch ci"
    )
    .is_err());
}
