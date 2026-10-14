use super::protocol::Frame;
use std::collections::{HashMap, HashSet, VecDeque};

pub struct NotificationBuffer {
    threads: HashMap<String, VecDeque<Frame>>,
    ready: HashSet<String>,
    per_thread: usize,
    max_threads: usize,
    bytes: usize,
}

impl NotificationBuffer {
    pub fn new(per_thread: usize, max_threads: usize) -> Self {
        Self {
            threads: HashMap::new(),
            ready: HashSet::new(),
            per_thread,
            max_threads,
            bytes: 0,
        }
    }

    pub fn accept(&mut self, frame: Frame) -> Result<Option<Frame>, String> {
        let params = match &frame {
            Frame::Notification { params, .. } | Frame::ServerRequest { params, .. } => params,
            _ => return Ok(Some(frame)),
        };
        let Some(thread) = params.get("threadId").and_then(serde_json::Value::as_str) else {
            return Ok(Some(frame));
        };
        if self.ready.contains(thread) {
            return Ok(Some(frame));
        }
        let size = params.to_string().len();
        if self.bytes.saturating_add(size) > 8 * 1024 * 1024 {
            return Err("RPC pre-response byte buffer exhausted".into());
        }
        if !self.threads.contains_key(thread) && self.threads.len() >= self.max_threads {
            return Err("RPC pre-response thread buffer exhausted".into());
        }
        let queue = self.threads.entry(thread.to_owned()).or_default();
        if queue.len() >= self.per_thread {
            return Err("RPC pre-response notification buffer exhausted".into());
        }
        queue.push_back(frame);
        self.bytes += size;
        Ok(None)
    }

    pub fn release(&mut self, thread: &str) -> Vec<Frame> {
        self.ready.insert(thread.into());
        let frames: Vec<_> = self
            .threads
            .remove(thread)
            .unwrap_or_default()
            .into_iter()
            .collect();
        for frame in &frames {
            if let Frame::Notification { params, .. } | Frame::ServerRequest { params, .. } = frame
            {
                self.bytes = self.bytes.saturating_sub(params.to_string().len());
            }
        }
        frames
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

    #[test]
    fn codex_rpc_control_buffer_keeps_root_ready_and_retains_prebind_requests() {
        let mut buffer = NotificationBuffer::new(4, 3);
        let request = Frame::ServerRequest {
            id: super::super::protocol::RequestId::String("approval".into()),
            method: "item/commandExecution/requestApproval".into(),
            params: serde_json::json!({"threadId":"root"}),
        };
        assert_eq!(buffer.accept(request.clone()).unwrap(), None);
        assert_eq!(buffer.release("root"), [request]);
        buffer.release("child");
        assert_eq!(buffer.accept(event("root")).unwrap(), Some(event("root")));
        assert_eq!(buffer.accept(event("child")).unwrap(), Some(event("child")));
    }
}
