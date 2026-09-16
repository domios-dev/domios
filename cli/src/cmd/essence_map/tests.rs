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

    assert_eq!(summary.nodes, 57);
    assert_eq!(summary.vectors, 7);
    assert_eq!(summary.capabilities, 36);
    assert_eq!(summary.core, 12);
    assert_eq!(summary.clients, 2);
    assert_eq!(summary.runs_on, 67);
    assert_eq!(summary.feeds, 50);
    assert_eq!(summary.part_of, 11);
    assert_eq!(summary.client_of, 2);
    assert_eq!(validate(fixture.path(), &map, &decisions), Ok(summary));
}

#[test]
fn essence_map_integrity_rejects_service_layer_self_parent_edges() {
    let fixture = fixture_root();
    let map = fixture.path().join("essences/map.jsonl");

    let summary = generate(fixture.path(), &map).expect("generate map without self-parent");
    let records = super::read_map(&map).expect("read generated map");

    assert_eq!(summary.part_of, 11);
    assert!(!records.iter().any(|record| matches!(
        record,
        super::MapRecord::Edge(edge)
            if edge.edge_type == super::EdgeType::PartOf && edge.from == edge.to
    )));
}

#[test]
fn essence_map_integrity_validator_rejects_hand_authored_self_parent_edges() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read generated map");
    let malformed = original.replacen(
        "\"from\":\"core:host-state\",\"to\":\"core:service-layer\"",
        "\"from\":\"core:host-state\",\"to\":\"core:host-state\"",
        1,
    );
    fs::write(&map, malformed).expect("write self-parent map");

    let error = validate(fixture.path(), &map, &decisions).expect_err("self-parent fails");
    assert!(error.contains("self-parent cycle"));
}

#[test]
fn essence_map_integrity_normalizes_crlf_before_parsing_and_hashing() {
    let lf = fixture_root();
    let crlf = fixture_root();
    for name in ["CAPABILITIES.md", "CORE.md"] {
        let lf_path = lf.path().join("essences").join(name);
        let crlf_path = crlf.path().join("essences").join(name);
        let text = fs::read_to_string(&lf_path).expect("read fixture source");
        let normalized = text.replace("\r\n", "\n");
        fs::write(&lf_path, &normalized).expect("write LF fixture source");
        fs::write(crlf_path, normalized.replace('\n', "\r\n"))
            .expect("write CRLF fixture source");
    }

    let lf_map = lf.path().join("essences/map.jsonl");
    let crlf_map = crlf.path().join("essences/map.jsonl");
    generate(lf.path(), &lf_map).expect("generate LF map");
    generate(crlf.path(), &crlf_map).expect("generate CRLF map");

    assert_eq!(
        fs::read(lf_map).expect("read LF map"),
        fs::read(crlf_map).expect("read CRLF map")
    );
}

#[test]
fn essence_map_integrity_refreshes_stale_digests_without_changing_semantic_links() {
    let fixture = fixture_root();
    let map = fixture.path().join("essences/map.jsonl");
    generate(fixture.path(), &map).expect("generate initial map");
    let before = super::read_map(&map).expect("read initial map");
    let core = fixture.path().join("essences/CORE.md");
    let source = fs::read_to_string(&core).expect("read core source");
    fs::write(&core, format!("{source}\n<!-- integrity fixture -->\n"))
        .expect("change core source");

    generate(fixture.path(), &map).expect("refresh stale digests");
    let after = super::read_map(&map).expect("read refreshed map");
    let before_keys: std::collections::BTreeSet<_> = before
        .iter()
        .map(|record| match record {
            super::MapRecord::Node(node) => format!("node:{}", node.id),
            super::MapRecord::Edge(edge) => format!("edge:{}", super::edge_key(edge)),
        })
        .collect();
    let after_keys: std::collections::BTreeSet<_> = after
        .iter()
        .map(|record| match record {
            super::MapRecord::Node(node) => format!("node:{}", node.id),
            super::MapRecord::Edge(edge) => format!("edge:{}", super::edge_key(edge)),
        })
        .collect();

    assert_eq!(before_keys, after_keys);
    let before_core_digest = before
        .iter()
        .find_map(|record| match record {
            super::MapRecord::Node(node) if node.id == "core:service-layer" => {
                Some(node.source.sha256.clone())
            }
            _ => None,
        })
        .expect("initial service-layer node");
    let after_core_digest = after
        .iter()
        .find_map(|record| match record {
            super::MapRecord::Node(node) if node.id == "core:service-layer" => {
                Some(node.source.sha256.clone())
            }
            _ => None,
        })
        .expect("refreshed service-layer node");
    assert_ne!(before_core_digest, after_core_digest, "source digest must refresh");
}

#[test]
fn essence_map_contract_committed_artifacts_are_fresh() {
    let root = source_root();
    let map = root.join("essences/map.jsonl");
    let decisions = root.join("essences/decisions.jsonl");
    let summary = validate(&root, &map, &decisions).expect("committed map is source-fresh");

    assert_eq!(summary.nodes, 57);
    assert_eq!(summary.edges, 130);
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
fn essence_truth_model_preserves_capability_statuses_and_shipped_core_nodes() {
    let fixture = fixture_root();
    let (map, _, _) = valid_map(fixture.path());
    let records = super::read_map(&map).expect("read generated map");
    let nodes: std::collections::BTreeMap<_, _> = records
        .iter()
        .filter_map(|record| match record {
            super::MapRecord::Node(node) => Some((node.id.as_str(), node)),
            super::MapRecord::Edge(_) => None,
        })
        .collect();
    assert_eq!(nodes["capability:essence-anchored-code-map"].status.as_deref(), Some("planned"));
    assert_eq!(nodes["capability:agent-providers"].status.as_deref(), Some("stable; Cursor in rollout; Aider and Goose planned"));
    assert!(nodes.contains_key("core:general-agent"));
    assert!(nodes.contains_key("core:crash-recovery-supervisor"));
    assert!(records.iter().any(|record| matches!(record, super::MapRecord::Edge(edge) if edge.from == "capability:session-continuity" && edge.to == "core:crash-recovery-supervisor")));
}

#[test]
fn essence_truth_model_rejects_renamed_capability_slug() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    fs::write(&map, original.replace("capability:agent-providers", "capability:renamed-providers"))
        .expect("write renamed map");
    let error = validate(fixture.path(), &map, &decisions).expect_err("renamed slug fails");
    assert!(error.contains("missing node") || error.contains("dangling"));
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
    let (map, decisions, summary) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    let first_node = original.lines().next().expect("generated node").to_string();
    let mut overflow = original;
    // Derived from the live map so this case cannot drift as the essences grow.
    for _ in 0..(101 - summary.nodes) {
        overflow.push_str(&first_node);
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
