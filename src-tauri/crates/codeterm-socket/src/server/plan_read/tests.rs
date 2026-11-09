use super::*;

#[test]
fn full_export_linking_preserves_unlinked_names_order_and_rejects_malformed_records() {
    let plan = json!({"id":"p","tasks":[
        {"id":"a","name":"First","section_ids":[]},
        {"id":"b","name":"Linked","section_ids":["section"]},
        {"id":"c","name":"Duplicate"},
        {"id":"d","name":"Duplicate","section_ids":[]}
    ]});
    let result = exported_plan_with_linking(plan.clone()).unwrap();
    assert_eq!(
        result["_linking"],
        json!({"unlinked_count":3,"unlinked_tasks":["First","Duplicate","Duplicate"]})
    );
    assert_eq!(result["tasks"], plan["tasks"]);
    assert!(exported_plan_with_linking(json!({"tasks":{}})).is_err());
    assert!(exported_plan_with_linking(json!({"tasks":[{"name":"a","section_ids":42}]})).is_err());
}
