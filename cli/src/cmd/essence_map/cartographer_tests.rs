//! The decision journal a Cartographer writes: one record per change, one
//! summary per release, and a revert the owner can actually take.

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use tempfile::TempDir;

use super::journal::{self, RecordOutcome, KEEP_CHOICE, REVERT_CHOICE_PREFIX};
use super::{generate, sha256, validate};
use crate::{Cli, Commands};

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("create cartographer map fixture");
    let essences = temp.path().join("essences");
    fs::create_dir_all(&essences).expect("create fixture essences");
    let source_essences = source_root().join("essences");
    for entry in fs::read_dir(&source_essences).expect("list source essences") {
        let entry = entry.expect("read source essence entry");
        let source = entry.path();
        if entry.file_type().expect("inspect source essence").is_file()
            && source
                .extension()
                .is_some_and(|extension| extension == "md")
        {
            fs::copy(&source, essences.join(entry.file_name())).expect("copy source essence");
        }
    }
    temp
}

fn paths(root: &Path) -> (PathBuf, PathBuf) {
    (
        root.join("essences/map.jsonl"),
        root.join("essences/decisions.jsonl"),
    )
}

fn digest(path: &Path) -> String {
    sha256(&fs::read(path).expect("read map"))
}

/// Generate the map and seed the log with the first decision for it.
fn seeded(root: &Path) -> (PathBuf, PathBuf) {
    let (map, decisions) = paths(root);
    generate(root, &map).expect("generate fixture map");
    fs::write(&decisions, "").expect("create fixture decisions");
    let outcome = journal::record(
        &map,
        &decisions,
        "initial map",
        "generated from the source essences",
    )
    .expect("record the first decision");
    assert_eq!(outcome.decision_id(), "decision-0001");
    (map, decisions)
}

/// A source edit the generator must notice: every node carries the source
/// digest, so the map bytes change.
fn touch_source(root: &Path) {
    let core = root.join("essences/core.md");
    let text = fs::read_to_string(&core).expect("read fixture core essence");
    fs::write(&core, format!("{text}\n<!-- cartographer fixture -->\n")).expect("edit fixture");
}

#[test]
fn cartographer_map_cli_exposes_the_journal_actions_locally() {
    for argv in [
        vec![
            "codeterm",
            "essence-map",
            "record",
            "--summary",
            "s",
            "--reason",
            "r",
        ],
        vec!["codeterm", "essence-map", "release-summary"],
        vec!["codeterm", "essence-map", "revert", "--to", "decision-0001"],
    ] {
        let parsed = Cli::try_parse_from(argv).expect("journal action parses");
        assert!(matches!(parsed.command, Commands::EssenceMap { .. }));
        assert_eq!(
            crate::transport::transport_for(&parsed.command).local,
            crate::transport::Local::PreSocket
        );
    }
    assert!(Cli::try_parse_from(["codeterm", "essence-map", "record"]).is_err());
    assert!(Cli::try_parse_from(["codeterm", "essence-map", "revert"]).is_err());
}

#[test]
fn cartographer_map_records_a_decision_only_when_the_map_changed() {
    let temp = fixture_root();
    let root = temp.path();
    let (map, decisions) = seeded(root);

    // Regenerating the same sources reproduces the same map, so the boundary
    // is silent rather than logging a no-op.
    generate(root, &map).expect("regenerate");
    let unchanged = journal::record(&map, &decisions, "no change", "sources are identical")
        .expect("record over an unchanged map");
    assert_eq!(
        unchanged,
        RecordOutcome::Unchanged {
            id: "decision-0001".into(),
            map_sha256: digest(&map),
        }
    );
    assert_eq!(journal::read(&decisions).expect("read log").len(), 1);

    touch_source(root);
    generate(root, &map).expect("regenerate after the source edit");
    let appended = journal::record(
        &map,
        &decisions,
        "core essence edited",
        "the source changed",
    )
    .expect("record the change");
    assert!(matches!(appended, RecordOutcome::Appended { .. }));
    assert_eq!(appended.decision_id(), "decision-0002");

    let log = journal::read(&decisions).expect("read log");
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].map_sha256, digest(&map));
    validate(root, &map, &decisions).expect("the journal stays contract-valid");
}

#[test]
fn essence_map_integrity_journal_treats_crlf_map_as_the_same_semantic_map() {
    let temp = fixture_root();
    let root = temp.path();
    let (map, decisions) = seeded(root);
    let lf = fs::read_to_string(&map).expect("read generated LF map");
    fs::write(&map, lf.replace('\n', "\r\n")).expect("rewrite map as CRLF");

    let outcome = journal::record(&map, &decisions, "line ending only", "same semantic map")
        .expect("record CRLF map");
    assert!(matches!(outcome, RecordOutcome::Unchanged { .. }));
    assert_eq!(journal::read(&decisions).expect("read decisions").len(), 1);
}

#[test]
fn cartographer_map_release_summary_is_one_summary_with_one_revert_choice() {
    let temp = fixture_root();
    let root = temp.path();
    let (map, decisions) = seeded(root);

    for round in 0..2 {
        touch_source(root);
        generate(root, &map).expect("regenerate");
        journal::record(&map, &decisions, &format!("change {round}"), "source edit")
            .expect("record the change");
    }

    let release = journal::release_summary(&map, &decisions, Some("decision-0001"))
        .expect("compose the release summary");
    assert_eq!(release.baseline.as_deref(), Some("decision-0001"));
    assert_eq!(release.changes.len(), 2, "one summary covers every change");
    assert_eq!(
        release.current_map_sha256.as_deref(),
        Some(digest(&map).as_str())
    );
    let revert = format!("{REVERT_CHOICE_PREFIX}decision-0001");
    assert_eq!(
        release.choices,
        vec![KEEP_CHOICE.to_string(), revert.clone()],
        "the owner is offered one keep and one revert, never one per change"
    );
    assert_eq!(release.revert_choice(), Some(revert.as_str()));

    // A window with nothing in it asks the owner nothing.
    let quiet = journal::release_summary(&map, &decisions, Some("decision-0003"))
        .expect("compose an empty window");
    assert!(quiet.changes.is_empty());
    assert!(quiet.choices.is_empty());
    assert_eq!(quiet.revert_choice(), None);

    assert!(journal::release_summary(&map, &decisions, Some("decision-0099")).is_err());
}

#[test]
fn cartographer_map_revert_restores_the_recorded_map_and_stays_append_only() {
    let temp = fixture_root();
    let root = temp.path();
    let (map, decisions) = seeded(root);
    let baseline = digest(&map);

    touch_source(root);
    generate(root, &map).expect("regenerate");
    journal::record(&map, &decisions, "change", "source edit").expect("record the change");
    assert_ne!(digest(&map), baseline);
    let history = root
        .join(".codeterm/map-history")
        .join(format!("{baseline}.jsonl"));
    assert!(
        history.is_file(),
        "superseded map is retained outside essences"
    );
    assert!(!root.join("essences/map-history").exists());

    let outcome = journal::revert(&map, &decisions, "decision-0001").expect("take the revert");
    assert_eq!(outcome.restored, "decision-0001");
    assert_eq!(outcome.map_sha256, baseline);
    assert_eq!(outcome.decision, "decision-0003");
    assert_eq!(digest(&map), baseline, "the map is back to the baseline");

    let log = journal::read(&decisions).expect("read log");
    assert_eq!(log.len(), 3, "a revert appends; it never rewrites the log");
    assert_eq!(log[0].id, "decision-0001");
    assert_eq!(log[2].map_sha256, baseline);

    assert!(journal::revert(&map, &decisions, "decision-0099").is_err());
}
