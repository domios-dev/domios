use super::*;
use serde_json::json;

#[test]
fn surface_capture_keeps_nullable_keys_and_object_pairs() {
    let target = GrabTarget {
        attributes: vec![
            ("type".into(), "first".into()),
            ("type".into(), "submit".into()),
        ],
        ..Default::default()
    };
    let value = json!(GrabSelection {
        id: "g1".into(),
        label: "Button".into(),
        target
    });
    assert_eq!(value["target"]["attributes"], json!({"type":"submit"}));
    assert_eq!(value["target"]["styles"], json!({}));
    for key in ["react", "source"] {
        assert_eq!(value["target"].get(key), Some(&serde_json::Value::Null));
    }
    assert_eq!(
        value["target"]["accessibility"],
        json!({"role":null,"name":null})
    );
    let capture = SnipCapture {
        id: "s1".into(),
        width: 4,
        height: 8,
        thumbnail: None,
    };
    assert_eq!(
        json!(capture),
        json!({"id":"s1","width":4,"height":8,"thumbnail":null})
    );
}
