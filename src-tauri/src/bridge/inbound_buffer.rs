//! Per-pane FIFO buffer of inbound messages held during the agent_type=None gap.

use parking_lot::Mutex;
use std::collections::{BTreeMap, VecDeque};
use std::sync::OnceLock;

const MAX_QUEUED_PER_PANE: usize = 100;

fn buffers() -> &'static Mutex<BTreeMap<String, VecDeque<String>>> {
    static BUFFERS: OnceLock<Mutex<BTreeMap<String, VecDeque<String>>>> = OnceLock::new();
    BUFFERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn push(pane_id: &str, payload: String) {
    let mut map = buffers().lock();
    let q = map.entry(pane_id.to_string()).or_default();
    if q.len() >= MAX_QUEUED_PER_PANE {
        let dropped = q.pop_front().unwrap_or_default();
        log::warn!(
            "[bridge] inbound buffer full for pane={} (cap={}), dropping oldest: {}",
            pane_id,
            MAX_QUEUED_PER_PANE,
            dropped.chars().take(80).collect::<String>(),
        );
    }
    q.push_back(payload);
}

pub fn drain(pane_id: &str) -> Vec<String> {
    let mut map = buffers().lock();
    match map.remove(pane_id) {
        Some(q) => q.into_iter().collect(),
        None => Vec::new(),
    }
}

pub fn clear(pane_id: &str) {
    buffers().lock().remove(pane_id);
}

pub fn len(pane_id: &str) -> usize {
    buffers().lock().get(pane_id).map(|q| q.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_pane(prefix: &str) -> String {
        format!("{}-{}", prefix, uuid::Uuid::new_v4())
    }

    #[test]
    fn push_then_drain_preserves_fifo_order() {
        let p = unique_pane("fifo");
        push(&p, "a".into());
        push(&p, "b".into());
        push(&p, "c".into());
        let got = drain(&p);
        assert_eq!(got, vec!["a", "b", "c"]);
        assert_eq!(len(&p), 0);
    }

    #[test]
    fn drain_empty_returns_empty_vec() {
        let p = unique_pane("empty");
        assert!(drain(&p).is_empty());
    }

    #[test]
    fn clear_removes_all() {
        let p = unique_pane("clear");
        push(&p, "x".into());
        push(&p, "y".into());
        assert_eq!(len(&p), 2);
        clear(&p);
        assert_eq!(len(&p), 0);
        assert!(drain(&p).is_empty());
    }

    #[test]
    fn cap_drops_oldest_when_full() {
        let p = unique_pane("cap");
        for i in 0..(MAX_QUEUED_PER_PANE + 1) {
            push(&p, format!("m{}", i));
        }
        let got = drain(&p);
        assert_eq!(got.len(), MAX_QUEUED_PER_PANE);
        assert_eq!(got.first().map(String::as_str), Some("m1"));
        assert_eq!(
            got.last().map(String::as_str),
            Some(format!("m{}", MAX_QUEUED_PER_PANE).as_str()),
        );
    }

    #[test]
    fn distinct_panes_are_isolated() {
        let a = unique_pane("iso-a");
        let b = unique_pane("iso-b");
        push(&a, "from-a".into());
        push(&b, "from-b".into());
        let drained_a = drain(&a);
        let drained_b = drain(&b);
        assert_eq!(drained_a, vec!["from-a"]);
        assert_eq!(drained_b, vec!["from-b"]);
    }
}
