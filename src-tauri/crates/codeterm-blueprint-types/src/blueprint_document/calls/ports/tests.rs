use super::*;
use serde_json::{json, Value};

fn accepts_literal(value: Value, schema: Value) -> bool {
    compatible(&literal(&value), &serde_json::from_value(schema).unwrap())
}

#[test]
fn literal_array_objects_can_have_different_values_and_lengths() {
    assert!(accepts_literal(
        json!([{"name":"a","ok":true}, {"name":"longer","ok":false}]),
        json!({"type":"array","items":{"type":"object","required":["name","ok"],
            "additionalProperties":false,"properties":{"name":{"type":"string"},"ok":{"type":"boolean"}}}})
    ));
}

#[test]
fn literal_array_empty_satisfies_typed_items() {
    assert!(accepts_literal(
        json!([]),
        json!({"type":"array","items":{"type":"integer"}})
    ));
}

#[test]
fn literal_array_supports_different_optional_fields_and_unrestricted_items() {
    assert!(accepts_literal(
        json!([{"label":"a"}, {"count":2}]),
        json!({"type":"array","items":{"type":"object","additionalProperties":false,
            "properties":{"label":{"type":"string"},"count":{"type":"integer"}}}})
    ));
    assert!(accepts_literal(
        json!([1, "text", false, null, {}, []]),
        json!({"type":"array","items":{}})
    ));
}

#[test]
fn literal_array_checks_every_item_constraint() {
    let schema = json!({"type":"array","items":{"type":"object","required":["name","ok"],
        "additionalProperties":false,"properties":{"name":{"type":"string","maxLength":3},
        "ok":{"type":"boolean","enum":[true]}}}});
    for invalid in [
        json!({"name":"long","ok":true}),
        json!({"name":"ok","ok":false}),
        json!({"name":"ok","ok":"true"}),
        json!({"name":"ok"}),
        json!({"name":"ok","ok":true,"extra":1}),
    ] {
        assert!(!accepts_literal(
            json!([{"name":"a","ok":true}, invalid]),
            schema.clone()
        ));
    }
}

#[test]
fn literal_array_supports_nested_empty_arrays_and_numeric_widening() {
    assert!(accepts_literal(
        json!([[], [1, 2.5]]),
        json!({"type":"array","items":{"type":"array","items":{"type":"number"}}})
    ));
    assert!(!accepts_literal(
        json!([[], [1, "bad"]]),
        json!({"type":"array","items":{"type":"array","items":{"type":"number"}}})
    ));
}

#[test]
fn literal_array_preserves_whole_value_enum_and_media_checks() {
    assert!(!accepts_literal(
        json!([]),
        json!({"type":"array","enum":[[1]],"items":{"type":"integer"}})
    ));
    assert!(!accepts_literal(
        json!([]),
        json!({"type":"array","x-media":"text","items":{"type":"integer"}})
    ));
    assert!(!accepts_literal(json!([]), json!({"type":"object"})));
    let unbounded: Port = serde_json::from_value(json!({"type":"array"})).unwrap();
    let expected: Port =
        serde_json::from_value(json!({"type":"array","items":{"type":"integer"}})).unwrap();
    assert!(!compatible(&unbounded, &expected));
}
