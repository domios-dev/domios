use super::*;

#[test]
fn sent_busy_controls_can_await_stdout_only_after_declared_interactions_and_normal_input() {
    assert!(can_await_buffered_stdout(true, true, true, true, true));
    for missing in 0..5 {
        let mut facts = [true; 5];
        facts[missing] = false;
        assert!(!can_await_buffered_stdout(
            facts[0], facts[1], facts[2], facts[3], facts[4]
        ));
    }
}

#[test]
fn buffered_records_confirm_only_when_the_exact_stdout_lands_after_the_command_boundary() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let source = ModelSwitchTranscript {
        record_type: "system".into(),
        content_path: vec!["content".into()],
        prefix: "<stdout>".into(),
        suffix: "</stdout>".into(),
        ignored_content_regexes: Vec::new(),
        additional_record_sources: Vec::new(),
    };
    let pattern = Regex::new("^accepted choice$").unwrap();
    let record = serde_json::json!({"type":"system","content":"<stdout>accepted choice</stdout>"})
        .to_string();
    writeln!(file, "{record}").unwrap();
    let boundary = file.as_file().metadata().unwrap().len();
    assert!(!read_confirmation(file.path(), boundary, &source, &pattern));
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","content":"<stdout>accepted choice</stdout>"})
    )
    .unwrap();
    assert!(!read_confirmation(file.path(), boundary, &source, &pattern));
    writeln!(file, "{record}").unwrap();
    assert!(read_confirmation(file.path(), boundary, &source, &pattern));
}
