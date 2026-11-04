use super::*;

/// Six thousand ~1 KiB lines span more than one session parse chunk and record window.
fn multi_chunk_transcript() -> tempfile::NamedTempFile {
    let filler = "z".repeat(1000);
    let mut body = String::new();
    for index in 0..6000 {
        body.push_str(&format!(
            "{{\"type\":\"user\",\"text\":\"line-{index}-{filler}\"}}\n"
        ));
    }
    assert!(body.len() > 4 * 1024 * 1024, "fixture must span >1 chunk");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), body).unwrap();
    file
}

#[test]
fn a_failed_text_chunk_is_an_error_not_a_shorter_transcript() {
    let provider = load(
        MIN_MANIFEST,
        r#"
            let calls = 0;
            module.exports.default = {
                parseSessionMessages(text) {
                    if (++calls === 2) throw new Error('projection interrupted');
                    return text.split('\n').filter(Boolean).map(l => ({role:'user', text:l, isMeta:false}));
                }
            };
        "#,
    );
    let transcript = multi_chunk_transcript();
    let error = provider
        .parse_session_messages(transcript.path())
        .unwrap_err();
    assert!(error.contains("parseSessionMessages failed"), "{error}");
}

#[test]
fn a_failed_record_window_is_an_error_not_a_shorter_transcript() {
    let mut manifest: serde_json::Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["capabilities"]["recordSources"] =
        json!({"sessionTranscript":{"format":"jsonl","filter":{}}});
    let provider = load(
        &manifest.to_string(),
        r#"
            let calls = 0;
            module.exports.default = {
                parseSessionMessageRecords(records) {
                    if (++calls === 2) throw new Error('projection interrupted');
                    return records.map(r => ({role:'user', text:r.value.text, isMeta:false}));
                }
            };
        "#,
    );
    let transcript = multi_chunk_transcript();
    let error = provider
        .parse_session_messages(transcript.path())
        .unwrap_err();
    assert!(
        error.contains("parseSessionMessageRecords failed"),
        "{error}"
    );
}
