use axum::body::Bytes;
use std::collections::{HashMap, VecDeque};

pub(crate) const FRAME_CAP: usize = 64 * 1024;
pub(crate) const BYTE_BUDGET: usize = 4 * 1024 * 1024;
const INTERACTIVE_RESERVE: usize = 512 * 1024;
const TERMINAL_RESERVE: usize = 256 * 1024;
const CHANNEL_DEPTH: usize = 8;
const INTERACTIVE_BURST: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Traffic {
    Interactive,
    Bulk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Head,
    Data,
    End,
}

pub(crate) struct Frame {
    pub bytes: Bytes,
    pub kind: Kind,
}

struct Channel {
    traffic: Traffic,
    stream: bool,
    frames: VecDeque<Frame>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Push {
    Wait,
    Overflow,
    Closed,
    Oversized,
}

#[derive(Default)]
pub(crate) struct Model {
    channels: HashMap<String, Channel>,
    interactive: VecDeque<String>,
    bulk: VecDeque<String>,
    burst: usize,
    pub buffered: usize,
    closed: bool,
}

impl Model {
    pub fn register(&mut self, id: String, traffic: Traffic, stream: bool) -> bool {
        if self.closed || self.channels.len() >= 320 || self.channels.contains_key(&id) {
            return false;
        }
        self.channels.insert(
            id,
            Channel {
                traffic,
                stream,
                frames: VecDeque::new(),
            },
        );
        true
    }

    fn ready(&mut self, id: &str) {
        let Some(channel) = self.channels.get(id) else {
            return;
        };
        let Some(frame) = channel.frames.front() else {
            return;
        };
        if frame.kind != Kind::Data || channel.traffic == Traffic::Interactive {
            self.interactive.push_back(id.to_owned());
        } else {
            self.bulk.push_back(id.to_owned());
        }
    }

    pub fn push(&mut self, id: &str, frame: Frame) -> Result<(), (Push, Frame)> {
        if self.closed {
            return Err((Push::Closed, frame));
        }
        if frame.bytes.len() > FRAME_CAP {
            return Err((Push::Oversized, frame));
        }
        let Some(channel) = self.channels.get_mut(id) else {
            return Err((Push::Closed, frame));
        };
        let bulk = frame.kind == Kind::Data && channel.traffic == Traffic::Bulk;
        let budget = if frame.kind == Kind::End {
            BYTE_BUDGET
        } else if bulk {
            BYTE_BUDGET - INTERACTIVE_RESERVE
        } else {
            BYTE_BUDGET - TERMINAL_RESERVE
        };
        if channel.frames.len() >= CHANNEL_DEPTH || self.buffered + frame.bytes.len() > budget {
            return Err((
                if channel.stream {
                    Push::Overflow
                } else {
                    Push::Wait
                },
                frame,
            ));
        }
        let empty = channel.frames.is_empty();
        self.buffered += frame.bytes.len();
        channel.frames.push_back(frame);
        if empty {
            self.ready(id);
        }
        Ok(())
    }

    pub fn pop(&mut self) -> Option<Bytes> {
        let id = if !self.interactive.is_empty()
            && (self.burst < INTERACTIVE_BURST || self.bulk.is_empty())
        {
            self.burst = self.burst.saturating_add(1).min(INTERACTIVE_BURST);
            self.interactive.pop_front()?
        } else {
            self.burst = 0;
            self.bulk.pop_front()?
        };
        let channel = self.channels.get_mut(&id)?;
        let frame = channel.frames.pop_front()?;
        self.buffered -= frame.bytes.len();
        if frame.kind == Kind::End {
            self.remove(&id);
        } else {
            self.ready(&id);
        }
        Some(frame.bytes)
    }

    pub fn finish_overflow(&mut self, id: &str, end: Bytes) {
        let Some(channel) = self.channels.get_mut(id) else {
            return;
        };
        let head = if channel.frames.front().is_some_and(|f| f.kind == Kind::Head) {
            channel.frames.pop_front()
        } else {
            None
        };
        for frame in channel.frames.drain(..) {
            self.buffered -= frame.bytes.len();
        }
        if let Some(head) = head {
            channel.frames.push_back(head);
        }
        self.buffered += end.len();
        channel.frames.push_back(Frame {
            bytes: end,
            kind: Kind::End,
        });
        self.interactive.retain(|entry| entry != id);
        self.bulk.retain(|entry| entry != id);
        self.ready(id);
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(channel) = self.channels.remove(id) {
            for frame in channel.frames {
                self.buffered -= frame.bytes.len();
            }
        }
        self.interactive.retain(|entry| entry != id);
        self.bulk.retain(|entry| entry != id);
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.channels.clear();
        self.interactive.clear();
        self.bulk.clear();
        self.buffered = 0;
    }

    pub fn closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests;
