use super::protocol::Frame;
use std::collections::{HashMap, HashSet, VecDeque};

pub struct NotificationBuffer {
    threads: HashMap<String, VecDeque<Frame>>,
    ready: HashSet<String>,
    per_thread: usize,
    max_threads: usize,
}

impl NotificationBuffer {
    pub fn new(per_thread: usize, max_threads: usize) -> Self {
        Self {
            threads: HashMap::new(),
            ready: HashSet::new(),
            per_thread,
            max_threads,
        }
    }

    pub fn accept(&mut self, frame: Frame) -> Result<Option<Frame>, String> {
        let Frame::Notification { params, .. } = &frame else {
            return Ok(Some(frame));
        };
        let Some(thread) = params.get("threadId").and_then(serde_json::Value::as_str) else {
            return Ok(Some(frame));
        };
        if self.ready.contains(thread) {
            return Ok(Some(frame));
        }
        if !self.threads.contains_key(thread) && self.threads.len() >= self.max_threads {
            return Err("RPC pre-response thread buffer exhausted".into());
        }
        let queue = self.threads.entry(thread.to_owned()).or_default();
        if queue.len() >= self.per_thread {
            return Err("RPC pre-response notification buffer exhausted".into());
        }
        queue.push_back(frame);
        Ok(None)
    }

    pub fn release(&mut self, thread: &str) -> Vec<Frame> {
        self.ready.clear();
        self.ready.insert(thread.into());
        self.threads
            .remove(thread)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(thread: &str) -> Frame {
        Frame::Notification {
            method: "item/started".into(),
            params: serde_json::json!({"threadId":thread}),
        }
    }
    #[test]
    fn codex_rpc_buffer_preserves_order_and_fails_closed_on_overflow() {
        let mut buffer = NotificationBuffer::new(2, 1);
        assert_eq!(buffer.accept(event("a")).unwrap(), None);
        assert!(buffer.accept(event("b")).is_err());
        buffer.accept(event("a")).unwrap();
        assert!(buffer.accept(event("a")).is_err());
        assert_eq!(buffer.release("a"), vec![event("a"), event("a")]);
        assert_eq!(buffer.accept(event("a")).unwrap(), Some(event("a")));
    }
}
