use super::*;

#[test]
fn search_unit_facts_preserve_the_indexed_text_identifiers_and_status_contract() {
    let parent = UnitParent {
        id: "p",
        title: "Opaque λ",
        scope: "scope",
        status: "archived",
    };
    let header = parent.header("Root\nSecond", 17);
    assert_eq!(header.id, "planunit_p_plan_p");
    assert_eq!(header.body, "Plan \"Opaque λ\"\nRoot\nSecond");
    assert_eq!(header.updated_at, 17);
    let section = parent.section("s", "Nested", "section text", 2, 3, 17);
    assert_eq!(section.id, "planunit_p_plan_section_s");
    assert_eq!(
        section.body,
        "Plan \"Opaque λ\" · section 2/3: Nested\nsection text"
    );
    let criteria = vec!["one".into(), "two".into()];
    let task = parent.task(TaskUnitInput {
        id: "t",
        name: "Task",
        status: &TaskStatus::InReview,
        updated_at: u64::MAX,
        body: "body",
        acceptance_criteria: &criteria,
        external_refs: vec!["url-one", "url-two"],
    });
    assert_eq!(task.id, "planunit_p_task_t");
    assert_eq!(
        task.body,
        "Plan \"Opaque λ\" · task: Task\none\ntwo\nurl-one\nurl-two\nbody"
    );
    assert_eq!(task.status.as_deref(), Some("archived:in_review"));
    assert_eq!(task.updated_at, u64::MAX);
    let stride = parent.stride(
        "Task",
        "in_review",
        StrideUnitInput {
            id: "m",
            kind: "decision",
            text: "proof",
            ts: u64::MAX,
        },
    );
    assert_eq!(stride.id, "planunit_p_stride_m");
    assert_eq!(stride.title, "Task stride decision");
    assert_eq!(
        stride.body,
        "Plan \"Opaque λ\" · task \"Task\" · stride(decision): proof"
    );
    assert_eq!(stride.status.as_deref(), Some("archived:in_review"));
    assert_eq!(stride.updated_at, u64::MAX);
}
