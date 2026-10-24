use super::*;

fn prove_fanin(language: &str, first: &str, second: &str, join: &str) {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("fanin-stdin.json");
    let mut doc = document(language, first, &sink);
    let sink_node = doc["steps"][1].clone();
    doc["steps"] = json!([
        {"id":"left","code":{"lang":language,"src":first},"out":{"type":"integer"}},
        {"id":"right","code":{"lang":language,"src":second},"out":{"type":"string"}},
        {"id":"join","code":{"lang":language,"src":join},"bind":{"a":"left.out","b":"right.out"},"out":schema()},
        sink_node
    ]);
    doc["steps"][3]["bind"] = json!({"in":"join.out"});
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "fanin-proof");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    let envelope = ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap()).unwrap();
    assert_eq!(envelope.value, json!({"n":8}));
    let entries = journal(&handle, &lease.job.id);
    for name in ["left", "right", "join", "sink"] {
        assert!(
            entries
                .iter()
                .any(|e| e.step_name == name && e.state == JournalState::Done),
            "{diagnostic}"
        );
    }
    assert_eq!(handle.probe_owner.snapshot().unconfirmed, 0);
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_fanin_python_reaches_persisted_sink() {
    prove_fanin(
        "python",
        "def main(value): return 7",
        "def main(value): return 'x'",
        "def main(value): return {'n': value['a'] + len(value['b'])}",
    );
}

#[test]
fn blueprint_v2_runtime_fanin_ts_reaches_persisted_sink() {
    prove_fanin("ts", "export default function main(value: unknown) { return 7; }", "export default function main(value: unknown) { return 'x'; }", "export default function main(value: {a:number,b:string}) { return {n:value.a + value.b.length}; }");
}

#[test]
fn blueprint_v2_runtime_fanin_command_receives_named_envelope() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("command-fanin.json");
    let mut doc = document("python", "def main(value): return 7", &sink);
    let mut command = doc["steps"][1].clone();
    command.as_object_mut().unwrap().remove("in");
    command["bind"] = json!({"a":"left.out","b":"right.out"});
    doc["steps"] = json!([
        {"id":"left","code":{"lang":"python","src":"def main(value): return 7"},"out":{"type":"integer"}},
        {"id":"right","code":{"lang":"python","src":"def main(value): return 'linen'"},"out":{"type":"string"}},
        command
    ]);
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) =
        run_recorded(&handle, &fixture.service, &lease, "command-fanin-proof");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    let envelope = ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap()).unwrap();
    assert_eq!(envelope.value, json!({"a":7,"b":"linen"}));
    drop(handle);
    drop(fixture);
    close(storage);
}
