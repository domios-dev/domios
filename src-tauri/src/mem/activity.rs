use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemActivityEvent {
    pub op: String,
    pub mem_ids: Vec<String>,
    pub source: MemActivitySource,
    pub ts_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemActivitySource {
    pub kind: String,
    pub pane_id: Option<String>,
    pub worker_id: Option<String>,
    pub agent_provider: Option<String>,
}

pub struct ActivityRing {
    buf: Mutex<VecDeque<MemActivityEvent>>,
    cap: usize,
}

impl ActivityRing {
    pub fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            buf: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
        })
    }

    pub fn record(&self, event: MemActivityEvent) {
        let mut buf = self.buf.lock();
        if buf.len() >= self.cap {
            buf.pop_front();
        }
        buf.push_back(event);
    }

    pub fn recent(&self, since_ms: u64) -> Vec<MemActivityEvent> {
        self.buf
            .lock()
            .iter()
            .filter(|e| e.ts_ms >= since_ms)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<MemActivityEvent> {
        self.buf.lock().iter().cloned().collect()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: u64) -> MemActivityEvent {
        MemActivityEvent {
            op: "read".into(),
            mem_ids: vec!["id1".into()],
            source: MemActivitySource {
                kind: "rest".into(),
                pane_id: None,
                worker_id: None,
                agent_provider: None,
            },
            ts_ms: ts,
        }
    }

    #[test]
    fn ring_records_and_caps() {
        let ring = ActivityRing::new(500);
        for i in 0..600u64 {
            ring.record(ev(i));
        }
        let all = ring.all();
        assert_eq!(all.len(), 500);
        assert_eq!(all.first().unwrap().ts_ms, 100);
        assert_eq!(all.last().unwrap().ts_ms, 599);
    }

    #[test]
    fn ring_recent_filters_by_ts() {
        let ring = ActivityRing::new(500);
        ring.record(ev(1000));
        ring.record(ev(2000));
        ring.record(ev(3000));
        let recent = ring.recent(2000);
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().all(|e| e.ts_ms >= 2000));
    }

    #[test]
    fn serialize_roundtrip() {
        let e = MemActivityEvent {
            op: "write".into(),
            mem_ids: vec!["a".into(), "b".into()],
            source: MemActivitySource {
                kind: "worker".into(),
                pane_id: Some("p1".into()),
                worker_id: Some("w1".into()),
                agent_provider: Some("claude".into()),
            },
            ts_ms: 42,
        };
        let j = serde_json::to_string(&e).unwrap();
        let back: MemActivityEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(back.op, "write");
        assert_eq!(back.mem_ids, vec!["a", "b"]);
        assert_eq!(back.source.kind, "worker");
        assert_eq!(back.source.pane_id.as_deref(), Some("p1"));
        assert_eq!(back.ts_ms, 42);
    }
}
