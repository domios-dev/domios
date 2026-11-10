use crate::InstalledPluginRow;
use serde_json::{json, Value};

#[test]
fn plugin_rows_keep_absent_and_explicit_null_distinct() {
    let absent = InstalledPluginRow::default();
    let present: InstalledPluginRow = serde_json::from_value(
        json!({"id":"installed","source":null,"install_time":null,"load_error":null}),
    )
    .unwrap();
    let absent = json!(absent);
    let present = json!(present);
    for key in ["source", "install_time", "load_error"] {
        assert!(absent.get(key).is_none());
        assert_eq!(present.get(key), Some(&Value::Null));
    }
    for key in ["display_name", "version", "default_model"] {
        assert!(absent.get(key).is_none());
        assert!(present.get(key).is_none());
    }
}
