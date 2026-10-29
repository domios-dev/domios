use super::*;
use serde_json::json;

#[test]
fn cost_unit_host_sdk_shared_exact_decimal_fixtures_and_manifest_validation() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../packages/plugin-sdk/src/fixtures/cost-units.json"
    )))
    .unwrap();
    let units: CostUnits = serde_json::from_value(fixture["units"].clone()).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        assert_eq!(
            units
                .format(
                    case["amount"].as_u64().unwrap(),
                    case["unit"].as_str().unwrap()
                )
                .unwrap(),
            case["expected"].as_str().unwrap()
        );
    }
    let manifest = crate::js_runtime::manifest::PluginManifest::from_json_str(&json!({"id":"fixture-units","displayName":"Fixture","version":"0.1.0","capabilities":{"costUnits":fixture["units"]}}).to_string()).unwrap();
    assert_eq!(manifest.capabilities.cost_units, units);
    for invalid in [
        json!({"credit":{"one":"credit","other":"credits","currency":{"code":"usd","scale":6}}}),
        json!({"credit":{"one":"credit","other":"credits","currency":{"code":"USD","scale":16}}}),
        json!({"credit":{"one":"credit","other":"credits","extra":"untrusted"}}),
        json!({"credit":{"one":"credit\n","other":"credits"}}),
    ] {
        assert!(serde_json::from_value::<CostUnits>(invalid).is_err());
    }
    assert!(units.format(u64::MAX, "credits").is_err());
    assert!(units.format(1, "bad unit").is_err());
}
