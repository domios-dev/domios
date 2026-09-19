//! The human-readable reasons retained from the original essence coverage audit.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

const COVERAGE_PATH: &str = ".codeterm/essence-document-coverage.jsonl";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageReason {
    kind: String,
    document: String,
    reason: String,
}

fn coverage_reasons() -> Vec<CoverageReason> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let path = root.join(COVERAGE_PATH);
    let text = fs::read_to_string(&path).expect("read the document coverage reasons");
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("coverage reason line {}: {error}", index + 1))
        })
        .collect()
}

#[test]
fn cartographer_document_coverage_reasons_are_individual_and_nonempty() {
    let mut documents = BTreeSet::new();
    let mut reasons = BTreeSet::new();

    for row in coverage_reasons() {
        assert_eq!(row.kind, "coverage");
        assert!(
            documents.insert(row.document.clone()),
            "duplicate document: {}",
            row.document
        );
        assert!(!row.reason.trim().is_empty(), "{} has no reason", row.document);
        assert!(
            reasons.insert(row.reason.trim().to_owned()),
            "duplicate reason: {}",
            row.document
        );
    }
}
