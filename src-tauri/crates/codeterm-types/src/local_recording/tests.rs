use super::*;

#[test]
fn button_requires_exact_collection_and_bounded_unique_usages() {
    let mut button = LocalRecordingButton {
        interface_number: 0,
        collection_id: "col01".into(),
        report_id: 1,
        report_bytes: 2,
        usage_page: 9,
        press_usage: 1,
        button_usages: vec![1, 2],
        bit_field: 0,
        link_collection: 0,
        debounce_ms: 10,
    };
    assert!(button.valid());
    button.button_usages.push(1);
    assert!(!button.valid());
    button.button_usages = vec![2];
    assert!(!button.valid());
    button.button_usages = vec![1];
    button.collection_id = "colGG".into();
    assert!(!button.valid());
    button.collection_id = "COL01".into();
    button.report_bytes = 65;
    assert!(!button.valid());
}

#[test]
fn receiver_query_requires_matching_report_id_and_bounded_bytes() {
    let mut query = ReceiverStatusQuery {
        usage_page: 1,
        usage: 1,
        report_id: 7,
        request: vec![7, 0],
    };
    assert!(query.valid());
    query.request[0] = 8;
    assert!(!query.valid());
    query.request = vec![7; 65];
    assert!(!query.valid());
    query.request = vec![7];
    assert!(!query.valid());
}
