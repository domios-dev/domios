pub fn result(stdout: &[u8]) -> serde_json::Value {
    let envelope: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    let object = envelope
        .as_object()
        .expect("JSON envelope must be an object");
    assert_eq!(
        object.keys().map(String::as_str).collect::<Vec<_>>(),
        ["result"]
    );
    envelope["result"].clone()
}
