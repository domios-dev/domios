use super::*;

#[test]
fn only_the_exact_string_true_enables_purchases() {
    assert!(purchases_enabled_in(
        &json!({"entries":{"purchases_enabled":"true"},"state":"UPDATE"})
    ));
    for response in [
        json!({"entries":{"purchases_enabled":"false"},"state":"UPDATE"}),
        json!({"entries":{"purchases_enabled":"TRUE"}}),
        json!({"entries":{"purchases_enabled":" true"}}),
        json!({"entries":{"purchases_enabled":true}}),
        json!({"entries":{"purchases_enabled":"1"}}),
        json!({"entries":{}}),
        json!({"state":"NO_TEMPLATE"}),
        json!({"state":"EMPTY_CONFIG"}),
        json!(null),
        json!("true"),
    ] {
        assert!(!purchases_enabled_in(&response), "{response}");
    }
}
