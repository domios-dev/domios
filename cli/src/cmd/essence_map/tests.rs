use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use tempfile::TempDir;

use super::{generate_from_paths, sha256, validate, MapSummary};
use crate::args::essence_map::EssenceMapPaths;
use crate::{Cli, Commands};

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn copy_essences(essences: &Path) {
    fs::create_dir_all(essences).expect("create fixture essences");
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
}

fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("create essence map fixture");
    copy_essences(&temp.path().join("essences"));
    temp
}

fn generate(root: &Path, map: &Path) -> Result<super::MapSummary, String> {
    super::generate_with_data_root(root, map, &root.join("codeterm-data"))
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn node_ids(map: &Path) -> BTreeSet<String> {
    super::read_map(map)
        .expect("read generated map")
        .into_iter()
        .filter_map(|record| match record {
            super::MapRecord::Node(node) => Some(node.id),
            super::MapRecord::Edge(_) => None,
        })
        .collect()
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
fn essence_map_contract_accepts_valid_document_anchor_sets() {
    let fixture = fixture_root();
    let (map, decisions, summary) = valid_map(fixture.path());

    assert_eq!(summary.nodes, 61);
    assert_eq!(summary.vectors, 7);
    assert_eq!(summary.capabilities, 39);
    assert_eq!(summary.core, 13);
    assert_eq!(summary.clients, 2);
    assert_eq!(summary.runs_on, 77);
    assert_eq!(summary.feeds, 56);
    assert_eq!(summary.part_of, 12);
    assert_eq!(summary.client_of, 2);
    assert_eq!(validate(fixture.path(), &map, &decisions), Ok(summary));
}

#[test]
fn essence_map_contract_rejects_a_stable_doc_without_a_map_block() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    fs::write(fixture.path().join("essences/vision.md"), "# Vision\n")
        .expect("remove vision map block");

    let error = validate(fixture.path(), &map, &decisions).expect_err("missing map block fails");
    assert!(error.contains("vision.md"));
    assert!(error.contains("missing a fenced `map` block"));
}

#[test]
fn essence_map_contract_rejects_an_empty_anchor_list() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    fs::write(
        fixture.path().join("essences/vision.md"),
        "# Vision\n\n```map\nanchors:\n```\n",
    )
    .expect("empty vision anchors");

    let error = validate(fixture.path(), &map, &decisions).expect_err("empty anchors fail");
    assert!(error.contains("empty `anchors:` list"));
}

#[test]
fn essence_map_contract_rejects_an_unknown_document_anchor() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let vision = fixture.path().join("essences/vision.md");
    let contents = fs::read_to_string(&vision)
        .expect("read vision")
        .replace("\r\n", "\n");
    let changed = contents.replacen("anchors:\n", "anchors:\n  - capability:not-a-node\n", 1);
    fs::write(&vision, changed).expect("add unknown vision anchor");

    let error = validate(fixture.path(), &map, &decisions).expect_err("unknown anchor fails");
    assert!(error.contains("capability:not-a-node"));
    assert!(error.contains("unknown map anchor"));
}

#[test]
fn essence_map_integrity_rejects_service_layer_self_parent_edges() {
    let fixture = fixture_root();
    let map = fixture.path().join("essences/map.jsonl");

    let summary = generate(fixture.path(), &map).expect("generate map without self-parent");
    let records = super::read_map(&map).expect("read generated map");

    assert_eq!(summary.part_of, 12);
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
    for name in ["capabilities.md", "core.md"] {
        let lf_path = lf.path().join("essences").join(name);
        let crlf_path = crlf.path().join("essences").join(name);
        let text = fs::read_to_string(&lf_path).expect("read fixture source");
        let normalized = text.replace("\r\n", "\n");
        fs::write(&lf_path, &normalized).expect("write LF fixture source");
        fs::write(crlf_path, normalized.replace('\n', "\r\n")).expect("write CRLF fixture source");
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
    let core = fixture.path().join("essences/core.md");
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
    assert_ne!(
        before_core_digest, after_core_digest,
        "source digest must refresh"
    );
}

#[test]
fn essence_map_contract_committed_artifacts_are_fresh() {
    let root = source_root();
    let map = root.join("essences/map.jsonl");
    let decisions = root.join("essences/decisions.jsonl");
    let summary = validate(&root, &map, &decisions).expect("committed map is source-fresh");

    assert_eq!(summary.nodes, 61);
    assert_eq!(summary.edges, 147);
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
    assert_eq!(
        nodes["capability:essence-anchored-code-map"]
            .status
            .as_deref(),
        Some("planned")
    );
    assert_eq!(
        nodes["capability:agent-providers"].status.as_deref(),
        Some("stable; Cursor in rollout; Aider and Goose planned")
    );
    assert!(nodes.contains_key("core:general-agent"));
    assert!(nodes.contains_key("core:crash-recovery-supervisor"));
    assert!(records.iter().any(|record| matches!(record, super::MapRecord::Edge(edge) if edge.from == "capability:session-continuity" && edge.to == "core:crash-recovery-supervisor")));
}

#[test]
fn essence_truth_model_rejects_renamed_capability_slug() {
    let fixture = fixture_root();
    let (map, decisions, _) = valid_map(fixture.path());
    let original = fs::read_to_string(&map).expect("read map");
    fs::write(
        &map,
        original.replace("capability:agent-providers", "capability:renamed-providers"),
    )
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
    let capabilities = fixture.path().join("essences/capabilities.md");
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

#[test]
fn essence_map_stable_ids_survive_a_display_rename() {
    let fixture = fixture_root();
    let (map, _, _) = valid_map(fixture.path());
    let capabilities = fixture.path().join("essences/capabilities.md");
    let source = fs::read_to_string(&capabilities).expect("read capabilities");
    let renamed = source
        .replace(
            "Agents live and work in tabs {#vector:agents-live-and-work-in-tabs}",
            "Agents dwell in tabs {#vector:agents-live-and-work-in-tabs}",
        )
        .replace(
            "| capability:agent-providers | Agent providers |",
            "| capability:agent-providers | Model providers |",
        );
    assert_ne!(renamed, source, "the fixture rename must apply");
    fs::write(&capabilities, renamed).expect("write renamed source");

    generate(fixture.path(), &map).expect("regenerate after the rename");
    let ids = node_ids(&map);
    assert!(ids.contains("vector:agents-live-and-work-in-tabs"));
    assert!(ids.contains("capability:agent-providers"));
    let label = super::read_map(&map)
        .expect("read regenerated map")
        .into_iter()
        .find_map(|record| match record {
            super::MapRecord::Node(node) if node.id == "capability:agent-providers" => {
                Some(node.label)
            }
            _ => None,
        })
        .expect("renamed capability node");
    assert_eq!(label, "Model providers");
}

#[test]
fn essence_map_migrates_label_derived_ids_to_authored_ids() {
    let fixture = fixture_root();
    let capabilities = fixture.path().join("essences/capabilities.md");
    let source = fs::read_to_string(&capabilities).expect("read capabilities");
    // The authored id deliberately differs from the label's slug, so a node id
    // can only come from metadata, never from the heading or display text.
    let migrated = source.replace(
        "| capability:team-observer | Team observer |",
        "| capability:local-observer | Team observer |",
    );
    assert_ne!(migrated, source, "the fixture migration must apply");
    fs::write(&capabilities, migrated).expect("write migrated source");

    let map = fixture.path().join("essences/map.jsonl");
    generate(fixture.path(), &map).expect("generate from authored ids");
    let ids = node_ids(&map);
    assert!(ids.contains("capability:local-observer"));
    assert!(!ids.contains("capability:team-observer"));
}

#[test]
fn essence_map_rejects_a_relation_to_an_unknown_stable_id() {
    let fixture = fixture_root();
    let capabilities = fixture.path().join("essences/capabilities.md");
    let source = fs::read_to_string(&capabilities).expect("read capabilities");
    let changed = source.replace(
        "| client:ui | capability:message-delivery |",
        "| client:ui | capability:missing |",
    );
    assert_ne!(changed, source, "the dangling reference must apply");
    fs::write(&capabilities, changed).expect("write dangling source");

    let error = generate(fixture.path(), &fixture.path().join("essences/map.jsonl"))
        .expect_err("a dangling stable id fails");
    assert!(error.contains("capability:missing"));
}

#[test]
fn essence_map_requires_explicit_id_metadata() {
    let fixture = fixture_root();
    let capabilities = fixture.path().join("essences/capabilities.md");
    let source = fs::read_to_string(&capabilities).expect("read capabilities");
    let changed = source.replace("{#vector:agents-live-and-work-in-tabs}", "");
    assert_ne!(changed, source, "the id metadata removal must apply");
    fs::write(&capabilities, changed).expect("write source without id metadata");

    let error = generate(fixture.path(), &fixture.path().join("essences/map.jsonl"))
        .expect_err("a missing stable id fails");
    assert!(error.contains("{#"));
}

#[test]
fn essence_map_represents_a_clientless_library_topology() {
    let fixture = tempfile::tempdir().expect("create library fixture");
    let essences = fixture.path().join("essences");
    fs::create_dir_all(&essences).expect("create library essences");
    fs::write(
        essences.join("capabilities.md"),
        "# Capabilities\n\n\
         ## 1. A vector {#vector:a-vector}\n\n\
         | ID | Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |\n\
         |---|---|---|---|---|---|---|---|\n\
         | capability:reader | Reader | Reads input | CLI | stays pure | stable | core:library | capability:writer |\n\
         | capability:writer | Writer | Writes output | CLI | stays pure | stable | core:library | — (sink) |\n",
    )
    .expect("write library capabilities");
    fs::write(
        essences.join("core.md"),
        "# Library core\n\n\
         ## Core parts\n\n\
         ### Library {#core:library}\n\n\
         - Root: true.\n\
         - Invariants: stays a library.\n",
    )
    .expect("write library core");

    let map = essences.join("map.jsonl");
    let summary = generate(fixture.path(), &map).expect("generate a clientless map");
    assert_eq!(summary.clients, 0);
    assert_eq!(summary.client_of, 0);
    assert_eq!(summary.core, 1);
    assert_eq!(summary.nodes, 4);
}

#[test]
fn essence_map_refuses_a_linked_worktree() {
    let fixture = tempfile::tempdir().expect("create git fixture");
    let canonical = fixture.path().join("canonical");
    let worktree = fixture.path().join("worktree");
    fs::create_dir(&canonical).expect("create canonical checkout");
    run_git(&canonical, &["init", "--quiet"]);
    run_git(
        &canonical,
        &[
            "-c",
            "user.name=Map Test",
            "-c",
            "user.email=map-test@example.invalid",
            "commit",
            "--allow-empty",
            "--message",
            "map fixture",
        ],
    );
    run_git(
        &canonical,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().expect("worktree path is UTF-8"),
            "HEAD",
        ],
    );
    copy_essences(&canonical.join("essences"));

    let canonical_paths = EssenceMapPaths {
        root: canonical.clone(),
        map: None,
        decisions: None,
    };
    generate_from_paths(&canonical_paths).expect("the canonical checkout is accepted");

    let worktree_paths = EssenceMapPaths {
        root: worktree.clone(),
        map: None,
        decisions: None,
    };
    let error = generate_from_paths(&worktree_paths).expect_err("a linked worktree is refused");
    assert!(error.contains("linked worktree"));
}
