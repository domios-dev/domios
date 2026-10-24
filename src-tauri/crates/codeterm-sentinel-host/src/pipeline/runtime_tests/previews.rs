use super::*;

#[test]
fn journal_previews_redact_secrets_without_changing_the_executed_value() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("secret-sink.json");
    let mut doc = document("python", "def main(value): return value", &sink);
    doc["trigger"]["source"] = json!({"lit":{"text":"{\"password\":\"opaque-canary\"}"}});
    for step in doc["steps"].as_array_mut().unwrap() {
        step["in"] = json!({"type":"object","properties":{"password":{"type":"string"}},"required":["password"]});
        step["out"] = step["in"].clone();
    }
    doc["steps"][1].as_object_mut().unwrap().remove("out");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) =
        run_recorded(&handle, &fixture.service, &lease, "secret-preview");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    let envelope = ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap()).unwrap();
    assert_eq!(envelope.value, json!({"password":"opaque-canary"}));
    let entries = journal(&handle, &lease.job.id);
    let transform = entries
        .iter()
        .find(|row| row.step_name == "transform" && row.state == JournalState::Done)
        .unwrap();
    for (side, port) in [("inputs", "in"), ("outputs", "out")] {
        assert_eq!(
            transform.evidence["ports"][side][port]["preview"],
            "[redacted]"
        );
        assert_eq!(transform.evidence["ports"][side][port]["redacted"], true);
    }
    assert!(!serde_json::to_string(&entries)
        .unwrap()
        .contains("opaque-canary"));
    drop(handle);
    drop(fixture);
    close(storage);
}
