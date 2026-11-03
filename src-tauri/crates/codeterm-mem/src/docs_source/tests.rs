use super::{search, search_with_hybrid_start};
use std::collections::BTreeSet;

#[test]
fn disabled_hybrid_and_zero_limit_do_not_start_vector_builds() {
    let disabled = search_with_hybrid_start("memory", 3, false, || {
        panic!("disabled hybrid must not start vector work")
    });
    assert_eq!(disabled, search("memory", 3));
    assert!(search_with_hybrid_start("memory", 0, true, || {
        panic!("zero-limit search must not start vector work")
    })
    .is_empty());
}

#[test]
fn enabled_hybrid_search_requests_background_vectors() {
    let started = std::cell::Cell::new(false);
    let _ = search_with_hybrid_start("memory", 3, true, || started.set(true));
    assert!(started.get());
}

use super::{
    chunks_for_cli_reference, chunks_for_document, terms, DocSection, Index, CLI_REFERENCE, DOCS,
};

fn command_prefix_pairs(sections: &[DocSection]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for parent in sections {
        let prefix = format!("{} ", parent.section_title);
        for child in sections {
            if child.section_title.starts_with(&prefix) {
                pairs.push((parent.section_title.clone(), child.section_title.clone()));
            }
        }
    }
    pairs
}

fn generated_cli_heading_prefix_pairs() -> Vec<(String, String)> {
    let sections = super::chunks_for_headings("CLI", CLI_REFERENCE, 4, 4, false, true);
    command_prefix_pairs(&sections)
}

#[test]
fn headings_split_sections_deterministically_and_ignore_fences() {
    let text = "# Guide\nOverview text.\n## First section\nFirst body.\n```md\n## Not a section\n```\n### Nested section\nNested body.\n## First section\nLast body.";
    let first = chunks_for_document("GUIDE", text);
    let second = chunks_for_document("GUIDE", text);
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].section_title, "First section");
    assert_eq!(
        first[0].heading_path,
        vec!["Guide".to_string(), "First section".to_string()]
    );
    assert_eq!(first[0].anchor, "first-section");
    assert!(first[0].text.contains("Overview text."));
    assert!(first[0].text.contains("## Not a section"));
    assert_eq!(first[1].section_title, "Nested section");
    assert_eq!(
        first[1].heading_path,
        vec![
            "Guide".to_string(),
            "First section".to_string(),
            "Nested section".to_string()
        ]
    );
    assert_eq!(first[2].anchor, "first-section-1");
}

#[test]
fn bm25_returns_the_matching_test_section_first() {
    let index = Index::from_sections(vec![
        DocSection {
            doc_name: "CONFIG",
            section_title: "Secrets and unlock".into(),
            anchor: "secrets-and-unlock".into(),
            heading_path: vec!["Secrets and unlock".into()],
            text: "The vault secret is unlocked with the stored key.".into(),
        },
        DocSection {
            doc_name: "MEMORY",
            section_title: "Search memory".into(),
            anchor: "search-memory".into(),
            heading_path: vec!["Search memory".into()],
            text: "Memory candidates are recalled from the database.".into(),
        },
    ]);

    let result = index.search("vault unlock secret", 1);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].doc_name, "CONFIG");
    assert_eq!(result[0].anchor, "secrets-and-unlock");
}

#[test]
fn docs_vectors_flag_off_is_bm25_identical() {
    for query in ["domios sessions search", "secret unlock", "unmatched query"] {
        assert_eq!(
            super::search_with_hybrid(query, 5, false),
            super::search(query, 5)
        );
    }
}

#[test]
fn embedded_doc_names_match_the_cli_registry() {
    const CLI_DOCS: &str = include_str!("../../../../../cli/src/cmd/docs.rs");
    let registry = CLI_DOCS
        .split_once("pub(crate) const DOCS: &[EmbeddedDoc] = &[")
        .and_then(|(_, tail)| tail.split_once("\n];"))
        .map(|(registry, _)| registry)
        .expect("CLI docs registry");
    let cli_names = registry
        .split("doc!(")
        .skip(1)
        .map(|entry| {
            entry
                .trim_start()
                .strip_prefix('"')
                .and_then(|name| name.split_once('"'))
                .map(|(name, _)| name)
                .expect("registered doc name")
        })
        .collect::<Vec<_>>();
    let daemon_names = DOCS.iter().map(|doc| doc.name).collect::<Vec<_>>();
    assert_eq!(daemon_names, cli_names);
}

#[test]
fn generated_cli_reference_is_an_extra_command_chunk_corpus() {
    assert!(DOCS.iter().all(|doc| doc.name != "CLI"));
    let source_pairs = generated_cli_heading_prefix_pairs();
    assert!(source_pairs.iter().any(|(parent, child)| {
        parent == "domios plan task" && child == "domios plan task add"
    }));
    assert!(!source_pairs.iter().any(|(parent, child)| {
        parent == "domios plan task add" && child == "domios plan task attach"
    }));

    let sections = chunks_for_cli_reference(CLI_REFERENCE);
    assert!(!sections.is_empty());
    assert!(sections.iter().all(|section| section.doc_name == "CLI"));
    assert!(sections
        .iter()
        .all(|section| section.section_title.starts_with("domios")));
    let offending_pairs = command_prefix_pairs(&sections);
    if !offending_pairs.is_empty() {
        println!("offending generated CLI heading pairs: {offending_pairs:#?}");
    }
    assert!(
        offending_pairs.is_empty(),
        "offending generated CLI heading pairs: {offending_pairs:#?}"
    );
    let titles: BTreeSet<_> = sections
        .iter()
        .map(|section| section.section_title.clone())
        .collect();
    assert!(!titles.contains("domios"));
    assert!(!titles.contains("domios plan task"));
    assert!(titles.contains("domios plan task add"));
    assert!(titles.contains("domios plan task attach"));
    assert!(titles.contains("domios sessions get"));
    assert!(titles.contains("domios sessions search"));

    let result = Index::from_sections(sections).search("domios sessions search", 1);

    assert_eq!(result[0].doc_name, "CLI");
    assert_eq!(result[0].section_title, "domios sessions search");
}

#[test]
fn query_tokenization_is_case_insensitive() {
    assert_eq!(terms("Unlock Secret"), terms("unlock secret"));
}
