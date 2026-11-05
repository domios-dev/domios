use axum::body::Bytes;
use std::collections::{HashMap, VecDeque};

pub(crate) const FRAME_CAP: usize = 64 * 1024;
pub(crate) const BYTE_BUDGET: usize = 4 * 1024 * 1024;
const INTERACTIVE_RESERVE: usize = 512 * 1024;
const TERMINAL_RESERVE: usize = 256 * 1024;
const CHANNEL_DEPTH: usize = 8;
const INTERACTIVE_BURST: usize = 8;
/// Bulk RPC payload bytes the writer may hand to the socket before the client credits them: large enough
/// for a remote bandwidth-delay product; at most this much bulk per session can sit in kernel buffers ahead
/// of an interactive frame.
pub(crate) const BULK_WINDOW: u64 = 4 * 1024 * 1024;

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
    /// Payload bytes this frame costs against the bulk window; the encoded frame carries the same value.
    pub credit: u64,
}

pub(crate) struct Channel {
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
    bulk_sent: u64,
    bulk_credit: u64,
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

    fn bulk_open(&self) -> bool {
        self.bulk_sent.saturating_sub(self.bulk_credit) < BULK_WINDOW
    }

    /// Cumulative credited bytes the client has received; stale or repeated credits are no-ops.
    pub fn credit(&mut self, received: u64) {
        self.bulk_credit = self.bulk_credit.max(received.min(self.bulk_sent));
    }

    pub fn pop(&mut self) -> Option<Bytes> {
        // A closed window holds only credited RPC data; bulk streams keep their round-robin turn.
        let open = self.bulk_open();
        let bulk_pick = self
            .bulk
            .iter()
            .position(|id| open || self.channels.get(id).is_some_and(|channel| channel.stream));
        let id = if !self.interactive.is_empty()
            && (self.burst < INTERACTIVE_BURST || bulk_pick.is_none())
        {
            self.burst = self.burst.saturating_add(1).min(INTERACTIVE_BURST);
            self.interactive.pop_front()?
        } else if let Some(position) = bulk_pick {
            self.burst = 0;
            self.bulk.remove(position)?
        } else {
            return None;
        };
        let channel = self.channels.get_mut(&id)?;
        let frame = channel.frames.pop_front()?;
        if Self::credited(channel, &frame) {
            self.bulk_sent += frame.credit;
        }
        self.buffered -= frame.bytes.len();
        if frame.kind == Kind::End {
            self.remove(&id);
        } else {
            self.ready(&id);
        }
        Some(frame.bytes)
    }

    /// Frames that count against the bulk window; the writer tags exactly these with `credit`.
    pub(crate) fn credited(channel: &Channel, frame: &Frame) -> bool {
        frame.credit > 0
            && frame.kind == Kind::Data
            && channel.traffic == Traffic::Bulk
            && !channel.stream
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
            credit: 0,
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
