use super::*;
use crate::socket::server::plan_fields::optional_string_field;

#[test]
fn optional_string_field_preserves_missing_vs_null() {
    let params = json!({
        "model": "opus",
        "why": null
    });

    assert_eq!(optional_string_field(Some(&params), "missing"), None);
    assert_eq!(optional_string_field(Some(&params), "why"), Some(None));
    assert_eq!(
        optional_string_field(Some(&params), "model"),
        Some(Some("opus"))
    );
}
