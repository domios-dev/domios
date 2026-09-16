//! The Cartographer document audit: every markdown essence carries one verdict,
//! either mapped onto generated nodes or kept outside the map with its own
//! reason. The map itself stays generated; this only checks the audit over it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{build_inventory, MAX_NODE_COUNT};

const COVERAGE_PATH: &str = "essences/document-coverage.jsonl";
const ESSENCES_DIR: &str = "essences";
const VERDICT_MAPPED: &str = "mapped";
const VERDICT_OUTSIDE: &str = "outside";
const NODE_PREFIXES: [&str; 4] = ["vector:", "capability:", "core:", "client:"];

/// The two documents the generator parses. They anchor every node by
/// construction, so they are excluded from the per-node document budget below.
const GENERATOR_SOURCES: [&str; 2] = ["essences/CAPABILITIES.md", "essences/CORE.md"];

/// How many detail documents one node may carry before the audit is attaching
/// documents in bulk instead of judging them one by one.
const MAX_DETAIL_DOCUMENTS_PER_ANCHOR: usize = 3;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageRow {
    kind: String,
    document: String,
    verdict: String,
    anchors: Vec<String>,
    reason: String,
}

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn coverage_rows() -> Vec<CoverageRow> {
    let path = source_root().join(COVERAGE_PATH);
    let text = fs::read_to_string(&path).expect("read the document coverage audit");
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            let row: CoverageRow = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("coverage line {}: {error}", index + 1));
            assert_eq!(row.kind, "coverage", "line {} is not a coverage record", index + 1);
            row
        })
        .collect()
}

fn collect_documents(directory: &Path, root: &Path, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).expect("read an essences directory") {
        let path = entry.expect("read an essences entry").path();
        if path.is_dir() {
            collect_documents(&path, root, found);
            continue;
        }
        if path.extension().is_some_and(|extension| extension == "md") {
            let relative = path
                .strip_prefix(root)
                .expect("essence document lives under the root")
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(relative);
        }
    }
}

fn essence_documents() -> BTreeSet<String> {
    let root = source_root();
    let mut found = BTreeSet::new();
    collect_documents(&root.join(ESSENCES_DIR), &root, &mut found);
    found
}

#[test]
fn cartographer_document_coverage_gives_every_essence_document_one_verdict() {
    let rows = coverage_rows();
    let documents = essence_documents();

    let mut audited = BTreeSet::new();
    for row in &rows {
        assert!(
            audited.insert(row.document.clone()),
            "{} carries more than one verdict",
            row.document
        );
        assert!(
            row.verdict == VERDICT_MAPPED || row.verdict == VERDICT_OUTSIDE,
            "{} has unknown verdict `{}`",
            row.document,
            row.verdict
        );
    }

    let missing: Vec<&String> = documents.difference(&audited).collect();
    let orphaned: Vec<&String> = audited.difference(&documents).collect();
    assert!(missing.is_empty(), "essence documents without a verdict: {missing:?}");
    assert!(orphaned.is_empty(), "verdicts without an essence document: {orphaned:?}");
    assert_eq!(rows.len(), documents.len());
}

#[test]
fn cartographer_document_coverage_mapped_documents_point_at_generated_nodes() {
    let inventory = build_inventory(&source_root()).expect("build the source inventory");
    let node_ids: BTreeSet<&str> = inventory.nodes.iter().map(|node| node.id.as_str()).collect();

    let mapped: Vec<CoverageRow> = coverage_rows()
        .into_iter()
        .filter(|row| row.verdict == VERDICT_MAPPED)
        .collect();
    assert!(!mapped.is_empty(), "the audit maps no document at all");

    for row in &mapped {
        assert!(
            !row.anchors.is_empty(),
            "{} is mapped but names no anchor",
            row.document
        );
        let mut seen = BTreeSet::new();
        for anchor in &row.anchors {
            assert!(
                NODE_PREFIXES.iter().any(|prefix| anchor.starts_with(prefix)),
                "{} anchors `{anchor}`, which names no vector, capability, core part or client",
                row.document
            );
            assert!(
                node_ids.contains(anchor.as_str()),
                "{} anchors `{anchor}`, which the generated inventory does not contain",
                row.document
            );
            assert!(
                seen.insert(anchor.as_str()),
                "{} repeats anchor `{anchor}`",
                row.document
            );
        }
    }
}

#[test]
fn cartographer_document_coverage_excluded_documents_name_a_reason_and_no_node() {
    for row in coverage_rows()
        .iter()
        .filter(|row| row.verdict == VERDICT_OUTSIDE)
    {
        assert!(
            row.anchors.is_empty(),
            "{} is kept outside the map but still claims {:?}",
            row.document,
            row.anchors
        );
        assert!(
            !row.reason.trim().is_empty(),
            "{} is excluded without a reason",
            row.document
        );
    }
}

#[test]
fn cartographer_document_coverage_reasons_are_individual_not_a_template() {
    let rows = coverage_rows();
    let mut reasons: BTreeMap<String, &str> = BTreeMap::new();
    for row in &rows {
        assert!(
            !row.reason.trim().is_empty(),
            "{} carries an empty reason",
            row.document
        );
        if let Some(other) = reasons.insert(row.reason.trim().to_string(), &row.document) {
            panic!("{} and {} share one reason", other, row.document);
        }
    }
}

#[test]
fn cartographer_document_coverage_never_bulk_attaches_documents_to_one_node() {
    let mut per_anchor: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in coverage_rows()
        .into_iter()
        .filter(|row| !GENERATOR_SOURCES.contains(&row.document.as_str()))
    {
        for anchor in row.anchors {
            per_anchor.entry(anchor).or_default().push(row.document.clone());
        }
    }
    for (anchor, documents) in &per_anchor {
        assert!(
            documents.len() <= MAX_DETAIL_DOCUMENTS_PER_ANCHOR,
            "`{anchor}` absorbs {} documents in bulk: {documents:?}",
            documents.len()
        );
    }
}

#[test]
fn cartographer_document_coverage_stays_within_the_node_budget() {
    let inventory = build_inventory(&source_root()).expect("build the source inventory");
    assert!(
        inventory.nodes.len() <= MAX_NODE_COUNT,
        "the audit left {} nodes, over the budget of {MAX_NODE_COUNT}",
        inventory.nodes.len()
    );

    let anchored: BTreeSet<String> = coverage_rows()
        .into_iter()
        .flat_map(|row| row.anchors)
        .collect();
    let node_ids: BTreeSet<String> = inventory.nodes.iter().map(|node| node.id.clone()).collect();
    let invented: Vec<&String> = anchored.difference(&node_ids).collect();
    assert!(
        invented.is_empty(),
        "the audit invented nodes the essences do not generate: {invented:?}"
    );
}
