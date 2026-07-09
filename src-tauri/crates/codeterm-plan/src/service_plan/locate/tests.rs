use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::NewTask;

const TASK_YAML: &str = "id: task_1\nname: roundtrip\nstatus: pending\nbody: |-\n  First body line.\n  Second body line.\nrole_guidance: keep it\nmilestones: []\n";

const TASK_YAML_NULL_BODY: &str = "id: task_2\nname: empty\nbody: null\nstatus: pending\n";

// serde field order: id first, body after tags.
const PLAN_YAML_SERDE: &str = "id: pl_1\ntitle: T\nsections:\n- id: sec_parent\n  title: Parent\n  handle: parent\n  tags:\n  - ux\n  - backend\n  body: |-\n    Parent body A.\n    Parent body B.\n  subsections:\n  - id: sec_child\n    title: Child\n    tags: []\n    body: |-\n      Child body.\n    subsections: []\nstatus: active\n";

// python migration order: body BEFORE id inside the item.
const SPEC_YAML_PY: &str = "id: sp_1\nplan_id: pl_1\nsections:\n- attachments: []\n  body: |-\n    Requirement text line 1.\n    Requirement text 2.\n  canvas:\n    collapsed: false\n  handle: reqs\n  id: sec_req\n  subsections: []\n  tags: []\n  title: Requirements\n- attachments: []\n  body: |-\n    Second section body.\n  id: sec_two\n  title: Two\n";

fn slice(text: &str, start: usize, end: usize) -> Vec<&str> {
    text.lines().collect::<Vec<_>>()[start - 1..end].to_vec()
}

#[test]
fn task_body_block_range() {
    let (s, e, editable) = top_level_body_range(TASK_YAML).unwrap();
    assert!(editable);
    assert_eq!(
        slice(TASK_YAML, s, e),
        vec!["  First body line.", "  Second body line."]
    );
}

#[test]
fn task_null_body_points_at_key_not_editable() {
    let (s, e, editable) = top_level_body_range(TASK_YAML_NULL_BODY).unwrap();
    assert!(!editable);
    assert_eq!(s, e);
    assert_eq!(slice(TASK_YAML_NULL_BODY, s, e), vec!["body: null"]);
}

#[test]
fn section_body_after_tags_list_serde_order() {
    let (s, e, editable) = section_body_range(PLAN_YAML_SERDE, "sec_parent").unwrap();
    assert!(editable);
    assert_eq!(
        slice(PLAN_YAML_SERDE, s, e),
        vec!["    Parent body A.", "    Parent body B."]
    );
}

#[test]
fn nested_subsection_body_resolves_to_child_not_parent() {
    let (s, e, _) = section_body_range(PLAN_YAML_SERDE, "sec_child").unwrap();
    assert_eq!(slice(PLAN_YAML_SERDE, s, e), vec!["      Child body."]);
}

#[test]
fn python_order_body_before_id_is_found() {
    let (s, e, editable) = section_body_range(SPEC_YAML_PY, "sec_req").unwrap();
    assert!(editable);
    assert_eq!(
        slice(SPEC_YAML_PY, s, e),
        vec!["    Requirement text line 1.", "    Requirement text 2."]
    );
}

#[test]
fn second_sibling_section_body_is_scoped() {
    let (s, e, _) = section_body_range(SPEC_YAML_PY, "sec_two").unwrap();
    assert_eq!(slice(SPEC_YAML_PY, s, e), vec!["    Second section body."]);
}

#[test]
fn missing_section_is_none() {
    assert!(section_body_range(PLAN_YAML_SERDE, "sec_nope").is_none());
}

#[test]
fn locate_body_end_to_end_on_real_store_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Locate".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    let sec = store
        .add_section(
            &plan.id,
            "Phase",
            "Section body line one.\nSection body line two.",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "locate me".into(),
                body: "Task body line one.\nTask body line two.".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let loc = store
        .locate_body(&plan.id, LocateTarget::Task(&task))
        .unwrap();
    let text = std::fs::read_to_string(&loc.file).unwrap();
    assert_eq!(
        slice(&text, loc.start_line, loc.end_line),
        vec!["  Task body line one.", "  Task body line two."]
    );

    let loc = store
        .locate_body(&plan.id, LocateTarget::PlanSection(&sec))
        .unwrap();
    let text = std::fs::read_to_string(&loc.file).unwrap();
    assert_eq!(
        slice(&text, loc.start_line, loc.end_line),
        vec!["    Section body line one.", "    Section body line two."]
    );
    assert!(loc.file.ends_with("plan.yaml"));
}
