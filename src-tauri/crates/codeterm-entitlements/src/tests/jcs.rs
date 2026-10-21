use crate::jcs::{encode, es_number, JcsError};
use serde_json::{json, Value};

const PRODUCER_JCS_VECTORS: &str = include_str!("../../vectors/jcs-vectors.json");

#[test]
fn producer_jcs_vectors_match_verbatim() {
    let vectors: Vec<Value> = serde_json::from_str(PRODUCER_JCS_VECTORS).unwrap();
    assert_eq!(vectors.len(), 3);
    for vector in vectors {
        assert_eq!(
            encode(&vector["input"]).unwrap(),
            vector["encoded"].as_str().unwrap()
        );
    }
}

#[test]
fn numbers_follow_ecmascript_serialization() {
    let cases: [(f64, &str); 16] = [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (-1.5, "-1.5"),
        (0.1, "0.1"),
        (4.5, "4.5"),
        (0.002, "0.002"),
        (0.000001, "0.000001"),
        (1e-7, "1e-7"),
        (1.23e-18, "1.23e-18"),
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1e30, "1e+30"),
        (333333333.3333333, "333333333.3333333"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
    ];
    for (value, expected) in cases {
        assert_eq!(es_number(value).unwrap(), expected, "{value:e}");
    }
    assert_eq!(es_number(f64::NAN), Err(JcsError::NonFinite));
    assert_eq!(es_number(f64::INFINITY), Err(JcsError::NonFinite));
}

#[test]
fn integers_beyond_the_safe_range_serialize_as_doubles() {
    assert_eq!(
        encode(&json!(9_007_199_254_740_991u64)).unwrap(),
        "9007199254740991"
    );
    assert_eq!(
        encode(&json!(-9_007_199_254_740_991i64)).unwrap(),
        "-9007199254740991"
    );
    assert_eq!(encode(&json!(u64::MAX)).unwrap(), "18446744073709552000");
    assert_eq!(
        encode(&json!(295_147_905_179_352_830_000.0f64)).unwrap(),
        "295147905179352830000"
    );
}

#[test]
fn strings_escape_like_json_stringify() {
    let value = json!("\u{1}\u{1f}\u{7f}\u{8}\u{c}\t\n\r\"\\/\u{2028}é😀");
    assert_eq!(
        encode(&value).unwrap(),
        "\"\\u0001\\u001f\u{7f}\\b\\f\\t\\n\\r\\\"\\\\/\u{2028}é😀\""
    );
}

#[test]
fn objects_sort_by_utf16_and_nest() {
    let value: Value =
        serde_json::from_str(r#"{"b":[1,{"z":null,"a":true}],"a":{},"😀":false,"￿":0}"#).unwrap();
    assert_eq!(
        encode(&value).unwrap(),
        r#"{"a":{},"b":[1,{"a":true,"z":null}],"😀":false,"￿":0}"#
    );
}
