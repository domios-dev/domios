use super::ChatMsg;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Hash, PartialEq, Eq)]
struct Stream {
    tab: String,
    session: Option<String>,
    nonce: Option<String>,
}

impl Stream {
    fn new(tab: &str, session: Option<&str>, nonce: Option<&str>) -> Self {
        Self {
            tab: tab.into(),
            session: session.map(str::to_owned),
            nonce: nonce.map(str::to_owned),
        }
    }
}

static SEQUENCES: LazyLock<Mutex<HashMap<Stream, i64>>> = LazyLock::new(Default::default);

pub fn seed_snapshot(tab: &str, session: Option<&str>, nonce: Option<&str>, rows: &[ChatMsg]) {
    let floor = rows
        .iter()
        .map(|row| row.seq.saturating_add(1))
        .max()
        .unwrap_or_default()
        .max(rows.len() as i64);
    let mut sequences = SEQUENCES.lock();
    let next = sequences
        .entry(Stream::new(tab, session, nonce))
        .or_default();
    *next = (*next).max(floor);
}

pub(super) fn assign(tab: &str, session: Option<&str>, nonce: Option<&str>, rows: &mut [ChatMsg]) {
    let mut sequences = SEQUENCES.lock();
    let next = sequences
        .entry(Stream::new(tab, session, nonce))
        .or_default();
    for row in rows {
        row.seq = *next;
        *next = next.saturating_add(1);
    }
}

pub(crate) fn clear_tab(tab: &str) {
    SEQUENCES.lock().retain(|stream, _| stream.tab != tab);
}

#[cfg(test)]
mod tests;
