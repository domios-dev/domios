use super::*;
use crate::plan::{OrchestratorRef, Plan};

#[test]
fn parser_maps_headings_to_section_bodies() {
    let parsed =
        parse_markdown_sections("# Plan\n\n## Backend\nDo A\n\n### API\nDo B\n\n## UI\nDo C");

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].title, "Backend");
    assert_eq!(parsed[0].body, "Do A\n");
    assert_eq!(parsed[0].children[0].title, "API");
    assert_eq!(parsed[1].body, "Do C");
}

#[test]
fn ingest_upserts_existing_section_by_handle() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_ingest".into();
    plan.sections.push(Section {
        id: "sec_existing".into(),
        title: "Draft backend".into(),
        handle: Some("backend".into()),
        tags: vec![],
        body: String::new(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    });
    store.save(&mut plan, 1).unwrap();

    let result = store
        .ingest_plan_sections("pl_ingest", "## Backend\nImplement the service.", 2)
        .unwrap();

    assert_eq!(result.sections.len(), 1);
    assert!(!result.sections[0].created);
    let plan = store.get("pl_ingest").unwrap();
    assert_eq!(plan.sections[0].id, "sec_existing");
    assert_eq!(plan.sections[0].title, "Backend");
    assert_eq!(plan.sections[0].body, "Implement the service.");
}
