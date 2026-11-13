use codeterm_team_package::contracts::{TemplateProvenance, TemplateRef};
use codeterm_team_package::{check_yaml, CheckOptions, Package};

use super::*;
use crate::service::team_exchange::DEFAULT_TEMPLATE;

fn package(version: &str, when: &str) -> Package {
    let text = format!(
        "name: ada/orchid-desk\nversion: {version}\ntitle: Orchid desk\ndescription: Grows rare orchids from seed.\nwhen:\n  - {when}\nroles:\n  orchestrator:\n    kind: manager\n    instructions: |\n      Lead the greenhouse.\n"
    );
    check_yaml(&text, &CheckOptions::default())
        .package
        .expect("probe package is valid")
}

fn first(mem: &MemManager, library: &TemplateLibrary, query: &str) -> Option<(String, String)> {
    recall_in(mem, library, query)
        .unwrap()
        .matches
        .first()
        .map(|summary| (summary.name.clone(), summary.version.clone()))
}

fn indexed(mem: &MemManager, name: &str) -> usize {
    indexed_ids(mem, Some(name)).unwrap().len()
}

#[test]
fn templates_are_recalled_through_mem_and_follow_save_update_delete_and_reindex() {
    let mem = MemManager::open_in_memory().expect("mem store");
    let dir = tempfile::tempdir().unwrap();
    let library = TemplateLibrary::new(dir.path().canonicalize().unwrap());
    let probe = ("ada/orchid-desk".to_string(), "1.0.0".to_string());

    library
        .save(
            &package("1.0.0", "germinate orchid seedlings"),
            TemplateProvenance::Saved,
        )
        .unwrap();
    reindex_in(&mem, &library).unwrap();
    assert_eq!(
        first(&mem, &library, "germinate seedlings"),
        Some(probe.clone()),
        "by when"
    );
    assert_eq!(
        first(&mem, &library, "Orchid desk"),
        Some(probe.clone()),
        "by title"
    );
    assert_eq!(
        first(&mem, &library, "rare orchids from seed"),
        Some(probe),
        "by description"
    );
    assert_eq!(indexed(&mem, DEFAULT_TEMPLATE), 1, "builtins are indexed");

    let recalled = recall_in(&mem, &library, "").unwrap();
    assert!(recalled.matches.is_empty());
    assert_eq!(
        recalled.fallback.name, DEFAULT_TEMPLATE,
        "the default is always offered"
    );
    assert_eq!(recalled.fallback.provenance, TemplateProvenance::Builtin);
    let nothing = recall_in(&mem, &library, "quantum chromodynamics").unwrap();
    assert!(nothing
        .matches
        .iter()
        .all(|summary| summary.name != "ada/orchid-desk"));
    assert_eq!(nothing.fallback.name, DEFAULT_TEMPLATE);

    library
        .save(
            &package("1.1.0", "repot mature orchids"),
            TemplateProvenance::Saved,
        )
        .unwrap();
    index_in(&mem, &library, "ada/orchid-desk").unwrap();
    assert_eq!(
        first(&mem, &library, "repot mature"),
        Some(("ada/orchid-desk".to_string(), "1.1.0".to_string()))
    );
    assert_eq!(
        indexed(&mem, "ada/orchid-desk"),
        1,
        "an update replaces in place"
    );

    reindex_in(&mem, &library).unwrap();
    reindex_in(&mem, &library).unwrap();
    assert_eq!(
        indexed(&mem, "ada/orchid-desk"),
        1,
        "reindex never duplicates"
    );
    assert_eq!(indexed(&mem, DEFAULT_TEMPLATE), 1);

    library
        .delete(&TemplateRef {
            name: "ada/orchid-desk".into(),
            version: None,
        })
        .unwrap();
    index_in(&mem, &library, "ada/orchid-desk").unwrap();
    assert_eq!(
        indexed(&mem, "ada/orchid-desk"),
        0,
        "a deleted template is archived"
    );
    assert!(recall_in(&mem, &library, "repot mature orchids")
        .unwrap()
        .matches
        .iter()
        .all(|summary| summary.name != "ada/orchid-desk"));

    library
        .save(
            &package("1.2.0", "graft orchid hybrids"),
            TemplateProvenance::Imported,
        )
        .unwrap();
    reindex_in(&mem, &library).unwrap();
    assert_eq!(
        indexed(&mem, "ada/orchid-desk"),
        1,
        "a re-imported template is indexed once"
    );
    let hit = recall_in(&mem, &library, "graft hybrids").unwrap();
    assert_eq!(hit.matches[0].provenance, TemplateProvenance::Imported);
}

#[test]
fn removed_builtins_disappear_from_plain_mem_search_after_reindex() {
    let mem = MemManager::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let library = TemplateLibrary::new(dir.path().canonicalize().unwrap());
    for name in ["release-train", "qa-sweep", "distributed-bets"] {
        let mut removed = package("1.0.0", "retired template probe");
        removed.document.name = format!("dev.domios/{name}");
        mem.save_with_context(
            item(&removed, TemplateProvenance::Builtin),
            true,
            WriteContext::system(ACTOR),
        )
        .unwrap();
    }
    let query = SearchQuery {
        query: "retired template probe".into(),
        ..Default::default()
    };
    assert_eq!(mem.search(&query).unwrap().len(), 3);
    reindex_in(&mem, &library).unwrap();
    reindex_in(&mem, &library).unwrap();
    assert!(mem.search(&query).unwrap().is_empty());
    assert_eq!(indexed(&mem, DEFAULT_TEMPLATE), 1);
}
