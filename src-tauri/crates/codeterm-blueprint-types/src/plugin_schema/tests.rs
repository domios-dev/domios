use super::*;
use serde_json::json;

#[test]
fn plugin_schema_closed_nested_models_validate_optional_nullable_arrays_and_bounds() {
    let schema = PluginSchema::from_value(&json!({"type":"object","properties":{
        "model":{"type":"string","maxLength":128},
        "settings":{"type":"object","properties":{"strength":{"type":"number","minimum":0,"maximum":1},"seed":{"type":["integer","null"],"minimum":0,"maximum":100}},"required":["strength"]},
        "references":{"type":"array","maxItems":4,"items":{"type":"object","properties":{"id":{"type":"string","maxLength":64},"weight":{"type":"number","minimum":0,"maximum":1}},"required":["id"]}}
    },"required":["model","settings"]})).unwrap();
    let valid = json!({"model":"fixture","settings":{"strength":0,"seed":null},"references":[{"id":"asset"}]});
    schema.validate_instance(&valid).unwrap();
    for value in [
        json!({"model":"fixture","settings":{"strength":1.1}}),
        json!({"model":"fixture","settings":{"strength":0,"extra":true}}),
        json!({"settings":{"strength":0}}),
    ] {
        assert!(schema.validate_instance(&value).is_err());
    }
    assert_eq!(
        serde_json::from_value::<PluginSchema>(schema.to_value()).unwrap(),
        schema
    );
}

#[test]
fn plugin_schema_discriminated_variants_reject_arbitrary_unions_and_cross_variant_fields() {
    let a = json!({"type":"object","properties":{"operation":{"type":"string","const":"generate"},"prompt":{"type":"string","maxLength":128}},"required":["operation","prompt"]});
    let b = json!({"type":"object","properties":{"operation":{"type":"string","const":"transform"},"reference":{"type":"string","maxLength":64}},"required":["operation","reference"]});
    let schema = PluginSchema::from_value(&json!({"oneOf":[a.clone(),b]})).unwrap();
    schema
        .validate_instance(&json!({"operation":"generate","prompt":"opaque"}))
        .unwrap();
    assert!(schema
        .validate_instance(&json!({"operation":"generate","prompt":"opaque","reference":"asset"}))
        .is_err());
    assert!(PluginSchema::from_value(&json!({"oneOf":[a.clone(),a]})).is_err());
    assert!(
        PluginSchema::from_value(&json!({"oneOf":[{"type":"string"},{"type":"number"}]})).is_err()
    );
}

#[test]
fn plugin_schema_refuses_unknown_keywords_open_objects_and_excess_depth_or_payload() {
    for schema in [
        json!({"$ref":"remote"}),
        json!({"type":"object","additionalProperties":true}),
        json!({"type":"array","items":{"type":"string"}}),
        json!({"type":"string","pattern":".*"}),
        json!({"type":"number","minimum":2,"maximum":1}),
    ] {
        assert!(PluginSchema::from_value(&schema).is_err());
    }
    let mut deep = json!({"type":"string"});
    for _ in 0..10 {
        deep = json!({"type":"array","items":deep,"maxItems":1});
    }
    assert!(PluginSchema::from_value(&deep).is_err());
    let scalar = PluginSchema::from_value(&json!({"type":"string"})).unwrap();
    assert!(scalar
        .validate_instance(&json!("x".repeat(MAX_VALUE_BYTES + 1)))
        .is_err());
}

#[test]
fn plugin_schema_number_and_default_contracts_are_shared_with_js_safe_integer_values() {
    let schema = PluginSchema::from_value(
        &json!({"type":"integer","minimum":0,"maximum":9_007_199_254_740_991u64}),
    )
    .unwrap();
    schema
        .validate_instance(&json!(9_007_199_254_740_991u64))
        .unwrap();
    assert!(schema
        .validate_instance(&json!(9_007_199_254_740_992u64))
        .is_err());
    assert!(
        PluginSchema::from_value(&json!({"type":"string","maxLength":1,"default":"long"})).is_err()
    );
    assert!(PluginSchema::from_value(&json!({"type":"integer","enum":["wrong"]})).is_err());
}
