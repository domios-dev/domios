use super::super::dedupe::{Thresholds, Verdict};
use super::super::fakes::WordEmbedder;
use super::super::index::Slot;
use super::*;
use codeterm_blueprint_types::blueprint_memory::{parse_document, Evidence};

const LOGS: &str = r#"schema_version: 2
kind: pipeline
name: log-rotate
note: Keep disks tidy.
trigger:
  type: schedule
  every:
    lit: 1d
steps:
- id: rotate
  run:
    lit: logrotate --force /etc/logrotate.conf
result:
  to: notify
"#;
const BACKUP: &str = r#"schema_version: 2
kind: pipeline
name: backup
note: Copy the database every night.
trigger:
  type: schedule
  every:
    lit: 1d
steps:
- id: dump
  uses: http
  with:
    url:
      lit: https://backup.example.com/run
result:
  to: notify
"#;

fn evidence() -> Evidence {
    Evidence {
        kept: true,
        successes: 1,
        ..Evidence::default()
    }
}

fn keep(mem: &MemManager, area: &str, yaml: &str, intents: &[&str]) -> String {
    let intents = intents
        .iter()
        .map(|intent| (*intent).to_owned())
        .collect::<Vec<_>>();
    mem.save_blueprint_without_merge(area, parse_document(yaml).unwrap(), &intents, evidence())
        .and_then(|saved| mem.index_blueprint(&saved.id).map(|_| saved.id))
        .unwrap()
}

#[test]
fn each_intent_gets_its_own_vector_and_recall_takes_the_best_one() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let logs = keep(
        &mem,
        "project",
        LOGS,
        &[
            "prune stale application output nightly",
            "compress yesterday journals",
            "free disk quota",
        ],
    );
    let backup = keep(
        &mem,
        "project",
        BACKUP,
        &["copy the production database somewhere safe"],
    );
    let vectors = crate::backend::sqlite::blueprint::index::read_vectors(
        &mem.conn.lock(),
        &logs,
        &mem.embedder.identity(),
    )
    .unwrap();
    let intents = vectors.slot(Slot::Intent).collect::<Vec<_>>();
    assert_eq!(intents.len(), 3);
    assert!(intents
        .iter()
        .all(|(_, vector)| vector.len() == mem.embedder.identity().dimension));
    assert_eq!(vectors.slot(Slot::Canon).count(), 1);
    let only_vectors = RecallConfig {
        weights: Weights {
            lexical: 0.0,
            canon: 0.0,
            ..RecallConfig::default().weights
        },
        fusion: Fusion::Score,
        ..RecallConfig::default()
    };
    let hits = mem
        .recall_blueprint_hits(
            "project",
            "compress yesterday journals",
            None,
            5,
            &only_vectors,
        )
        .unwrap();
    assert_eq!(hits[0].entry.id, logs);
    assert!((hits[0].score - 1.0).abs() < 1e-6, "{}", hits[0].score);
    assert!(hits[0]
        .why
        .contains("intent 1.00 \"compress yesterday journals\""));
    let hits = mem
        .recall_blueprint_hits(
            "project",
            "production database copy",
            None,
            5,
            &only_vectors,
        )
        .unwrap();
    assert_eq!(hits[0].entry.id, backup);
}

#[test]
fn recall_is_scoped_before_the_limit_and_empty_area_reads_every_project() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let local = keep(&mem, "project", LOGS, &["free disk quota"]);
    let foreign = keep(&mem, "elsewhere", LOGS, &["free disk quota"]);
    let scoped = mem
        .recall_blueprint_hits(
            "project",
            "free disk quota",
            None,
            1,
            &RecallConfig::default(),
        )
        .unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].entry.id, local);
    let all = mem
        .recall_blueprint_hits("", "free disk quota", None, 5, &RecallConfig::default())
        .unwrap();
    let mut ids = all
        .iter()
        .map(|hit| hit.entry.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    let mut expected = vec![local, foreign];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn structure_only_fuses_when_a_draft_is_given() {
    let mem = MemManager::open_in_memory().unwrap();
    let logs = keep(&mem, "project", LOGS, &["shared words here"]);
    let backup = keep(&mem, "project", BACKUP, &["shared words here"]);
    let config = RecallConfig {
        fusion: Fusion::Rrf,
        weights: Weights {
            structure: 2.0,
            ..RecallConfig::default().weights
        },
        ..RecallConfig::default()
    };
    let plain = mem
        .recall_blueprint_hits("project", "shared words", None, 5, &config)
        .unwrap();
    assert!(plain.iter().all(|hit| !hit.why.contains("structure")));
    let draft = crate::blueprint_wl::signature(&parse_document(BACKUP).unwrap());
    let drafted = mem
        .recall_blueprint_hits("project", "shared words", Some(&draft), 5, &config)
        .unwrap();
    assert_eq!(drafted[0].entry.id, backup);
    assert!(drafted[0].why.contains("structure 1.00"));
    assert!(drafted.iter().any(|hit| hit.entry.id == logs));
}

#[test]
fn dedupe_verdicts_keep_distinct_graphs_out_of_exact_and_structural() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let original = keep(&mem, "project", LOGS, &["free disk quota"]);
    let twin = keep(
        &mem,
        "project",
        &LOGS
            .replace("log-rotate", "trim-logs")
            .replace("logrotate --force", "find /var/log -delete"),
        &["reclaim disk space"],
    );
    let copy = keep(&mem, "project", LOGS, &["free disk quota"]);
    let other = keep(
        &mem,
        "project",
        BACKUP,
        &["copy the production database somewhere safe"],
    );
    let thresholds = Thresholds::default();
    assert_eq!(
        mem.compare_blueprints(&original, &copy, &thresholds)
            .unwrap()
            .verdict,
        Verdict::Exact
    );
    let structural = mem
        .compare_blueprints(&original, &twin, &thresholds)
        .unwrap();
    assert_eq!(structural.verdict, Verdict::StructuralTwin);
    assert!(structural.why.contains("same hash"), "{}", structural.why);
    let distinct = mem
        .compare_blueprints(&original, &other, &thresholds)
        .unwrap();
    assert!(!matches!(
        distinct.verdict,
        Verdict::Exact | Verdict::StructuralTwin
    ));
    let twins = mem
        .blueprint_twins("project", &original, &thresholds)
        .unwrap();
    assert!(twins.iter().any(|(id, _)| *id == copy));
    assert!(twins.iter().any(|(id, _)| *id == twin));
}

#[test]
fn lexical_recall_matches_words_shared_by_every_entry_of_a_small_library() {
    let mem = MemManager::open_in_memory().unwrap();
    let only = keep(&mem, "solo", LOGS, &["Echo readiness"]);
    let hits = mem
        .recall_blueprint_hits("solo", "echo", None, 5, &RecallConfig::default())
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.id, only);
    keep(&mem, "pair", LOGS, &["Echo readiness"]);
    keep(&mem, "pair", BACKUP, &["Echo valid watcher"]);
    assert_eq!(
        mem.recall_blueprint_hits("pair", "Echo", None, 5, &RecallConfig::default())
            .unwrap()
            .len(),
        2
    );
}
