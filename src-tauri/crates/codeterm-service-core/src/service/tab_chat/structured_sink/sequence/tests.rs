use super::*;
use crate::ports::host::UiEvents;
use serde_json::Value;

#[derive(Default)]
struct Events(Mutex<Vec<Value>>);

impl UiEvents for Events {
    fn emit(&self, _: &str, payload: Value) {
        self.0.lock().push(payload);
    }

    fn emit_to(&self, _: &str, event: &str, payload: Value) {
        self.emit(event, payload);
    }
}

fn snapshot(count: usize) -> Vec<ChatMsg> {
    (0..count)
        .map(|ordinal| ChatMsg {
            id: format!("snapshot-{ordinal}"),
            seq: ordinal as i64,
            ..Default::default()
        })
        .collect()
}

fn emit(ui: &Events, tab: &str, count: usize) {
    let rows = vec![ChatMsg::default(); count];
    super::super::emit_chat_msgs(ui, tab, &rows, None, Some("session"), Some("nonce"));
}

fn emitted_seqs(ui: &Events) -> Vec<i64> {
    ui.0.lock()
        .iter()
        .flat_map(|event| event["messages"].as_array().unwrap())
        .map(|row| row["seq"].as_i64().unwrap())
        .collect()
}

#[test]
fn structured_live_seq_follows_snapshot_across_emits() {
    let tab = "structured-seq-two-emits";
    let ui = Events::default();
    seed_snapshot(tab, Some("session"), Some("nonce"), &snapshot(7));
    emit(&ui, tab, 2);
    emit(&ui, tab, 1);
    assert_eq!(emitted_seqs(&ui), [7, 8, 9]);
    clear_tab(tab);
}

#[test]
fn structured_live_seq_reattach_advances_past_new_snapshot_without_rewinding() {
    let tab = "structured-seq-reattach";
    let ui = Events::default();
    seed_snapshot(tab, Some("session"), Some("nonce"), &snapshot(3));
    emit(&ui, tab, 1);
    seed_snapshot(tab, Some("session"), Some("nonce"), &snapshot(12));
    emit(&ui, tab, 2);
    seed_snapshot(tab, Some("session"), Some("nonce"), &snapshot(2));
    emit(&ui, tab, 1);
    assert_eq!(emitted_seqs(&ui), [3, 12, 13, 14]);
    clear_tab(tab);
}

#[test]
fn structured_live_seq_is_scoped_to_binding_and_respects_snapshot_max() {
    let tab = "structured-seq-binding";
    let ui = Events::default();
    let rows = [ChatMsg {
        seq: 40,
        ..Default::default()
    }];
    seed_snapshot(tab, Some("session"), Some("nonce"), &rows);
    emit(&ui, tab, 1);
    seed_snapshot(tab, Some("session"), Some("new-nonce"), &snapshot(5));
    super::super::emit_chat_msgs(
        &ui,
        tab,
        &[ChatMsg::default()],
        None,
        Some("session"),
        Some("new-nonce"),
    );
    emit(&ui, tab, 1);
    assert_eq!(emitted_seqs(&ui), [41, 5, 42]);
    clear_tab(tab);
}

#[test]
fn structured_live_seq_reports_only_fresh_unseeded_streams() {
    let tab = "structured-seq-unseeded";
    let mut rows = [ChatMsg::default()];
    assert!(assign(tab, Some("session"), Some("nonce"), &mut rows));
    assert_eq!(rows[0].seq, 0);
    assert!(!assign(tab, Some("session"), Some("nonce"), &mut rows));
    assert_eq!(rows[0].seq, 1);

    seed_snapshot(tab, Some("session"), Some("seeded-nonce"), &snapshot(5));
    assert!(!assign(
        tab,
        Some("session"),
        Some("seeded-nonce"),
        &mut rows
    ));
    assert_eq!(rows[0].seq, 5);
    assert!(assign(tab, Some("session"), None, &mut rows));
    assert_eq!(rows[0].seq, 0);
    clear_tab(tab);
}
