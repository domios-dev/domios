use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use tempfile::TempDir;

use super::{generate, sha256, validate, MapSummary};
use crate::{Cli, Commands};

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("create essence map fixture");
    let essences = temp.path().join("essences");
    fs::create_dir_all(&essences).expect("create fixture essences");
    for name in ["CAPABILITIES.md", "CORE.md"] {
        fs::copy(
            source_root().join("essences").join(name),
            essences.join(name),
        )
        .expect("copy source essence");
    }
    temp
}

fn paths(root: &Path) -> (PathBuf, PathBuf) {
    (
        root.join("essences/map.jsonl"),
        root.join("essences/decisions.jsonl"),
    )
}

#[test]
fn essence_map_contract_cli_is_local_and_has_generation_actions() {
    let generated = Cli::try_parse_from(["codeterm", "essence-map", "generate"])
        .expect("generate command parses");
    let validated = Cli::try_parse_from(["codeterm", "essence-map", "validate"])
        .expect("validate command parses");
    assert!(matches!(generated.command, Commands::EssenceMap { .. }));
    assert!(matches!(validated.command, Commands::EssenceMap { .. }));
    assert_eq!(
        crate::transport::transport_for(&generated.command).local,
        crate::transport::Local::PreSocket
    );
}

fn valid_map(root: &Path) -> (PathBuf, PathBuf, MapSummary) {
    let (map, decisions) = paths(root);
    let summary = generate(root, &map).expect("generate fixture map");
    let map_hash = sha256(&fs::read(&map).expect("read fixture map"));
    fs::write(
        &decisions,
        format!(
            "{{\"kind\":\"decision\",\"id\":\"decision-0001\",\"map_sha256\":\"{map_hash}\",\"summary\":\"initial map\",\"reason\":\"generated from source essences\"}}\n"
        ),
    )
    .expect("write fixture decisions");
    (map, decisions, summary)
}

#[test]
fn essence_map_contract_accepts_current_source_inventory() {
    let fixture = fixture_root();
    let (map, decisions, summary) = valid_map(fixture.path());

    assert_eq!(summary.nodes, 55);
    assert_eq!(summary.vectors, 7);
    assert_eq!(summary.capabilities, 36);
    assert_eq!(summary.core, 10);
    assert_eq!(summary.clients, 2);
    assert_eq!(summary.runs_on, 59);
    assert_eq!(summary.feeds, 50);
    assert_eq!(summary.part_of, 10);
    assert_eq!(summary.client_of, 2);
    assert_eq!(validate(fixture.path(), &map, &decisions), Ok(summary));
}

#[test]
fn essence_map_contract_committed_artifacts_are_fresh() {
    let root = source_root();
    let map = root.join("essences/map.jsonl");
    let decisions = root.join("essences/decisions.jsonl");
    let summary = validate(&root, &map, &decisions).expect("committed map is source-fresh");

    assert_eq!(summary.nodes, 55);
    assert_eq!(summary.edges, 121);
}

#[test]
fn essence_map_contract_generation_is_deterministic() {
    let fixture = fixture_root();
    let first = fixture.path().join("essences/first.jsonl");
    let second = fixture.path().join("essences/second.jsonl");

    generate(fixture.path(), &first).expect("generate first map");
    generate(fixture.path(), &second).expect("generate second map");

    assert_eq!(
        fs::read(&first).expect("read first map"),
        fs::read(&second).expect("read second map")
    );
}

#[test]
fn essence_map_contract_rejects_unknown_record_fields() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    let malformed = original.replacen("\"kind\":\"node\"", "\"kind\":\"node\",\"unknown\":true", 1);
    fs::write(&map, malformed).expect("write malformed map");

    let error = validate(fixture.path(), &map, &decisions).expect_err("unknown field fails");
    assert!(error.contains("malformed record"));
    assert!(error.contains("unknown field"));
}

#[test]
fn essence_map_contract_rejects_dangling_edges() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    let malformed = original.replacen(
        "\"to\":\"capability:agent-providers\"",
        "\"to\":\"capability:missing\"",
        1,
    );
    fs::write(&map, malformed).expect("write dangling map");

    let error = validate(fixture.path(), &map, &decisions).expect_err("dangling edge fails");
    assert!(error.contains("dangling target"));
}

#[test]
fn essence_map_contract_rejects_budget_overflow() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    let first_node = original.lines().next().expect("generated node");
    let mut overflow = original;
    for _ in 0..46 {
        overflow.push_str(first_node);
        overflow.push('\n');
    }
    fs::write(&map, overflow).expect("write over-budget map");

    let error = validate(fixture.path(), &map, &decisions).expect_err("budget fails");
    assert!(error.contains("101 nodes"));
    assert!(error.contains("limit of 100"));
}

#[test]
fn essence_map_contract_rejects_stale_source_references() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let capabilities = fixture.path().join("essences/CAPABILITIES.md");
    let mut source = fs::read_to_string(&capabilities).expect("read capabilities");
    source.push_str("\n<!-- source changed -->\n");
    fs::write(&capabilities, source).expect("change source essence");

    let error = validate(fixture.path(), &map, &decisions).expect_err("stale source fails");
    assert!(error.contains("stale"));
    assert!(error.contains("changed since map generation"));
}

#[test]
fn essence_map_contract_rejects_non_append_decisions() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let hash = sha256(&fs::read(&map).expect("read map"));
    fs::write(
        &decisions,
        format!(
            "{{\"kind\":\"decision\",\"id\":\"decision-0002\",\"map_sha256\":\"{hash}\",\"summary\":\"new\",\"reason\":\"change\"}}\n{{\"kind\":\"decision\",\"id\":\"decision-0001\",\"map_sha256\":\"{hash}\",\"summary\":\"old\",\"reason\":\"change\"}}\n"
        ),
    )
    .expect("write non-append decisions");

    let error = validate(fixture.path(), &map, &decisions).expect_err("decision order fails");
    assert!(error.contains("strictly increasing"));
}
