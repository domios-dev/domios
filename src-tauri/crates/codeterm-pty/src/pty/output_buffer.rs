use std::collections::HashMap;

struct ByteRing {
    bytes: Vec<u8>,
    head: usize,
    len: usize,
}

impl ByteRing {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: vec![0; capacity],
            head: 0,
            len: 0,
        }
    }

    fn write(&mut self, start: usize, data: &[u8]) {
        let first = data.len().min(self.bytes.len() - start);
        self.bytes[start..start + first].copy_from_slice(&data[..first]);
        self.bytes[..data.len() - first].copy_from_slice(&data[first..]);
    }

    fn append(&mut self, data: &[u8]) {
        let capacity = self.bytes.len();
        if capacity == 0 {
            return;
        }
        if data.len() >= capacity {
            self.bytes.copy_from_slice(&data[data.len() - capacity..]);
            self.head = 0;
            self.len = capacity;
            return;
        }
        self.write((self.head + self.len) % capacity, data);
        let excess = (self.len + data.len()).saturating_sub(capacity);
        self.head = (self.head + excess) % capacity;
        self.len = (self.len + data.len()).min(capacity);
    }

    fn prepend(&mut self, prefix: &[u8]) {
        let take = prefix.len().min(self.bytes.len() - self.len);
        if take == 0 {
            return;
        }
        self.head = (self.head + self.bytes.len() - take) % self.bytes.len();
        self.write(self.head, &prefix[prefix.len() - take..]);
        self.len += take;
    }

    fn copy_range(&self, from: usize, limit: usize) -> Vec<u8> {
        let len = self.len.saturating_sub(from).min(limit);
        let mut result = Vec::with_capacity(len);
        if len == 0 {
            return result;
        }
        let start = (self.head + from) % self.bytes.len();
        let first = len.min(self.bytes.len() - start);
        result.extend_from_slice(&self.bytes[start..start + first]);
        result.extend_from_slice(&self.bytes[..len - first]);
        result
    }
}

pub struct OutputRead {
    pub bytes: Vec<u8>,
    pub start: u64,
    pub total: u64,
    pub following_continuation: bool,
}

pub struct OutputBuffer {
    buffers: HashMap<String, ByteRing>,
    full_buffers: HashMap<String, ByteRing>,
    total_written: HashMap<String, u64>,
    max_size: usize,
    max_full: usize,
}

impl OutputBuffer {
    pub fn new(max_size: usize) -> Self {
        Self::new_with_caps(max_size, 2 * 1024 * 1024)
    }

    pub(super) fn new_with_caps(max_size: usize, max_full: usize) -> Self {
        Self {
            buffers: HashMap::new(),
            full_buffers: HashMap::new(),
            total_written: HashMap::new(),
            max_size,
            max_full,
        }
    }

    pub fn append(&mut self, pty_id: &str, data: &[u8]) {
        self.buffers
            .entry(pty_id.to_owned())
            .or_insert_with(|| ByteRing::new(self.max_size))
            .append(data);
        self.full_buffers
            .entry(pty_id.to_owned())
            .or_insert_with(|| ByteRing::new(self.max_full))
            .append(data);
        *self.total_written.entry(pty_id.to_owned()).or_default() += data.len() as u64;
    }

    pub fn prepend(&mut self, pty_id: &str, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.buffers
            .entry(pty_id.to_owned())
            .or_insert_with(|| ByteRing::new(self.max_size))
            .prepend(data);
        self.full_buffers
            .entry(pty_id.to_owned())
            .or_insert_with(|| ByteRing::new(self.max_full))
            .prepend(data);
        *self.total_written.entry(pty_id.to_owned()).or_default() += data.len() as u64;
    }

    pub fn copy(&self, pty_id: &str) -> Option<Vec<u8>> {
        self.buffers
            .get(pty_id)
            .map(|buffer| buffer.copy_range(0, usize::MAX))
    }

    pub fn copy_full(&self, pty_id: &str) -> Option<Vec<u8>> {
        self.full_buffers
            .get(pty_id)
            .map(|buffer| buffer.copy_range(0, usize::MAX))
    }

    pub fn full_len(&self, pty_id: &str) -> usize {
        self.full_buffers.get(pty_id).map_or(0, |buffer| buffer.len)
    }

    pub fn len(&self, pty_id: &str) -> usize {
        self.buffers.get(pty_id).map_or(0, |buffer| buffer.len)
    }

    pub fn get_text(&self, pty_id: &str) -> Option<String> {
        self.copy(pty_id)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn get_full_text(&self, pty_id: &str) -> Option<String> {
        self.copy_full(pty_id)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn total_written(&self, pty_id: &str) -> u64 {
        self.total_written.get(pty_id).copied().unwrap_or(0)
    }

    pub fn read_since_limited(&self, pty_id: &str, offset: u64, cap: usize) -> OutputRead {
        let total = self.total_written(pty_id);
        let start = offset
            .max(total.saturating_sub(self.full_len(pty_id) as u64))
            .min(total);
        let bytes = self
            .full_buffers
            .get(pty_id)
            .map_or_else(Vec::new, |buffer| {
                let retained_from = total.saturating_sub(buffer.len as u64);
                buffer.copy_range((start - retained_from) as usize, cap)
            });
        let following_continuation = self.full_buffers.get(pty_id).is_some_and(|buffer| {
            let next = (start - total.saturating_sub(buffer.len as u64)) as usize + bytes.len();
            next < buffer.len
                && buffer.bytes[(buffer.head + next) % buffer.bytes.len()] & 0xc0 == 0x80
        });
        OutputRead {
            bytes,
            start,
            total,
            following_continuation,
        }
    }

    pub fn read_since(&self, pty_id: &str, offset: u64) -> (Vec<u8>, u64) {
        let read = self.read_since_limited(pty_id, offset, usize::MAX);
        (read.bytes, read.total)
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.buffers.remove(pty_id);
        self.full_buffers.remove(pty_id);
        self.total_written.remove(pty_id);
    }
}

#[cfg(test)]
mod tests;
