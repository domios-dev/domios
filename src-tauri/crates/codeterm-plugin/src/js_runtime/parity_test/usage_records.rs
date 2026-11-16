use super::*;
use crate::token_usage::{self, UsageAccounting};

#[test]
fn usage_records_native_js_shared_fixtures() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../../../../plugins/provider-shared/tests/fixtures/usage-records.json"
    ))
    .unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let id = case["provider"].as_str().unwrap();
        let js = load_js(id);
        let record = &case["record"];
        let native = match case["normalizer"].as_str().unwrap() {
            "nested" => token_usage::nested::parse_record(record, 123),
            "cumulative" => token_usage::cumulative::parse_record(record, 123),
            "grouped" => token_usage::grouped::parse_record(record, 123),
            "none" => None,
            other => panic!("missing fixture oracle: {other}"),
        };
        assert_eq!(
            serde_json::to_value(&native).unwrap(),
            case["expected"],
            "native {id}"
        );
        assert_eq!(
            js.parse_usage_record(record, 123).unwrap(),
            native,
            "JS {id}"
        );
        assert_eq!(
            js.usage_accounting(),
            if id == "cursor" {
                UsageAccounting::Unreported
            } else {
                UsageAccounting::Transcript
            }
        );
    }
    let native = native_provider_fixture();
    assert_eq!(native.usage_accounting(), UsageAccounting::Transcript);
    assert_eq!(
        load_js("acp").usage_accounting(),
        UsageAccounting::RuntimeTurn
    );
}

#[test]
fn usage_records_unreported_providers_do_not_guess_usage() {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../plugins/provider-shared/tests/fixtures/usage-unreported.json"
    ))
    .unwrap();
    for case in evidence["providers"].as_array().unwrap() {
        let provider = load_inline_js(
            case["provider"].as_str().unwrap(),
            "module.exports.default = {};",
        );
        assert_eq!(provider.usage_accounting(), UsageAccounting::Unreported);
        assert_eq!(
            provider.parse_usage_record(&case["evidence"], 123).unwrap(),
            None
        );
    }
}

#[test]
fn usage_records_js_failure_is_not_absence() {
    let js = load_inline_js(
        "usage-broken",
        "module.exports.default = {parseUsageRecord() {throw new Error('unreadable usage')}};",
    );
    assert!(js.parse_usage_record(&serde_json::json!({}), 123).is_err());
}
