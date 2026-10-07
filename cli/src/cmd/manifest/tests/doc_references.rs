use super::*;
use crate::cmd::docs::{canonical_name, DOCS};
use std::collections::HashSet;

fn references(text: &str) -> Vec<String> {
    let pattern = regex::Regex::new(r"\bcodeterm[ \t]+docs[ \t]+([A-Za-z][A-Za-z0-9-]*)").unwrap();
    pattern
        .captures_iter(text)
        .map(|capture| capture[1].to_string())
        .collect()
}

fn unknown_references(text: &str) -> Vec<String> {
    references(text)
        .into_iter()
        .filter(|name| canonical_name(name).is_none())
        .collect()
}

fn unreferenced_documents<'a>(
    names: impl IntoIterator<Item = &'a str>,
    navigation: &str,
) -> Vec<&'a str> {
    let referenced = references(navigation)
        .iter()
        .filter_map(|name| canonical_name(name))
        .collect::<HashSet<_>>();
    names
        .into_iter()
        .filter(|name| !referenced.contains(name))
        .collect()
}

#[test]
fn prompt_and_skill_doc_references_resolve() {
    for (path, label) in prescription_corpus().into_iter().filter(|(_, label)| {
        label.starts_with("src-tauri/prompts/") || label.starts_with("skills/codeterm/")
    }) {
        let text = std::fs::read_to_string(path).unwrap();
        let missing = unknown_references(&text);
        assert!(
            missing.is_empty(),
            "{label} references missing documents: {missing:?}"
        );
    }
}

#[test]
fn registered_documents_have_a_gate_or_index_reference() {
    let gate = std::fs::read_to_string(repo_path("src-tauri/prompts/codeterm_cli.md")).unwrap();
    let index = std::fs::read_to_string(repo_path("skills/codeterm/references/docs.md")).unwrap();
    let missing =
        unreferenced_documents(DOCS.iter().map(|doc| doc.name), &format!("{gate}\n{index}"));
    assert!(
        missing.is_empty(),
        "registered documents missing from gate/index: {missing:?}"
    );
}

#[test]
fn doc_reference_checks_reject_broken_and_orphaned_targets() {
    assert_eq!(
        unknown_references("in prose `codeterm docs MISSING-DOCUMENT`, then continue"),
        ["MISSING-DOCUMENT"]
    );
    assert_eq!(
        unreferenced_documents(["CONFIG", "ROUTING"], "`codeterm docs CONFIG`"),
        ["ROUTING"]
    );
    assert!(unknown_references(
        "codeterm docs <NAME>; codeterm docs --help; codeterm docs SENTINELS"
    )
    .is_empty());
    assert_eq!(
        references("`codeterm docs config`. `codeterm docs API-CLI`"),
        ["config", "API-CLI"]
    );
}
