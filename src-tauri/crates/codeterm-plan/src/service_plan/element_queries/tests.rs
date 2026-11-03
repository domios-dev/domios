use super::*;
use crate::plan::{OrchestratorRef, Plan};
use serde_json::json;

fn store_with_elements() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Elements".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_elements".into();
    store.save(&mut plan, 1).unwrap();
    let element_id = store
        .add_element(
            &plan.id,
            "external.issue",
            json!({
                "title": "Fix launch",
                "body": "Investigate the failing launch path."
            }),
            vec!["task_1".into()],
            Some(10.0),
            Some(20.0),
            2,
        )
        .unwrap();
    store
        .add_element(
            &plan.id,
            "note",
            json!({ "text": "Follow-up note\n\nDetails" }),
            Vec::new(),
            None,
            None,
            3,
        )
        .unwrap();
    (dir, store, plan.id, element_id)
}

#[test]
fn list_elements_returns_generic_summaries() {
    let (_dir, store, plan_id, _element_id) = store_with_elements();

    let list = store
        .list_elements(&plan_id, Some("external.issue"))
        .unwrap();

    assert_eq!(list.elements.len(), 1);
    assert_eq!(list.elements[0].kind, "external.issue");
    assert_eq!(list.elements[0].title, "Fix launch");
    assert_eq!(list.elements[0].links, vec!["task_1"]);
    assert_eq!(list.elements[0].canvas.x, Some(10.0));
}

#[test]
fn show_element_returns_raw_content_and_markdown() {
    let (_dir, store, plan_id, element_id) = store_with_elements();

    let details = store.show_element(&plan_id, &element_id).unwrap();

    assert_eq!(details.title, "Fix launch");
    assert_eq!(
        details.element.content["body"],
        "Investigate the failing launch path."
    );
    assert_eq!(details.markdown, "Investigate the failing launch path.");
}

#[test]
fn title_falls_back_to_first_text_line() {
    let element = CanvasElement {
        id: "el_note".into(),
        kind: "note".into(),
        canvas: ElementCanvas::default(),
        content: json!({ "text": "\nFirst line\nSecond line" }),
        links: Vec::new(),
    };

    assert_eq!(element_title(&element), "First line");
}
