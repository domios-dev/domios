use super::tab_name;
use serde_json::json;

#[test]
fn reads_only_the_current_title() {
    assert_eq!(tab_name(&json!({"title":" Scout "})), "Scout");
    assert_eq!(
        tab_name(&json!({"title":"Scout","tab_title":"Old"})),
        "Scout"
    );
    assert_eq!(tab_name(&json!({"tab_title":"Old"})), "");
}
