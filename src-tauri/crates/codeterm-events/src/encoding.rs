use super::{EncodedFrame, SystemEvent};
use serde::Serialize;
use std::io::{self, Write};
use std::sync::Arc;

pub const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;

struct LimitedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("event payload limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn bounded_json(value: &impl Serialize, limit: usize) -> Option<String> {
    let mut writer = LimitedBytes {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).ok()?;
    String::from_utf8(writer.bytes).ok()
}

pub(super) struct PreparedEvent {
    event: SystemEvent,
    replay_event: SystemEvent,
    event_type: String,
    bytes: String,
}

impl PreparedEvent {
    pub(super) fn new(event: SystemEvent, boot: &str) -> Self {
        let event = if bounded_json(&event, MAX_EVENT_BYTES).is_some() {
            event
        } else {
            SystemEvent::Generic {
                event_type: "overflow".into(),
                data: serde_json::json!({"reason": "payload-limit", "boot": boot}),
            }
        };
        let (event_type, data) = match &event {
            SystemEvent::Generic { event_type, data } => (event_type.clone(), data.clone()),
            other => (
                other.type_name().to_owned(),
                serde_json::to_value(other).unwrap_or_default(),
            ),
        };
        let mut bytes = format!(
            "{{\"boot\":{},\"data\":{},\"event_type\":{},\"seq\":",
            serde_json::json!(boot),
            data,
            serde_json::json!(event_type)
        );
        bytes.reserve(21);
        Self {
            replay_event: event.clone(),
            event,
            event_type,
            bytes,
        }
    }

    pub(super) fn webhook_data(&self) -> (&'static str, serde_json::Value) {
        (
            self.event.type_name(),
            serde_json::to_value(&self.event).unwrap_or_default(),
        )
    }

    pub(super) fn stamp(mut self, seq: u64) -> (SystemEvent, SystemEvent, Arc<EncodedFrame>) {
        self.bytes.push_str(&seq.to_string());
        self.bytes.push('}');
        let size = self.bytes.len();
        let frame = Arc::new(EncodedFrame {
            seq,
            event_type: self.event_type,
            bytes: Arc::new(self.bytes),
            size,
        });
        (self.event, self.replay_event, frame)
    }
}
