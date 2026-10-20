use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use futures_core::Stream;
use futures_util::StreamExt;
use parking_lot::RwLock;
use serde::Serialize;

use super::stream_diagnostics::StreamDiagnostics;

pub(crate) static STREAMS: LazyLock<StreamCensus> = LazyLock::new(StreamCensus::default);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StreamKind {
    Events,
    Output,
    TabOutput,
    Surface,
}

impl StreamKind {
    const ALL: [Self; 4] = [Self::Events, Self::Output, Self::TabOutput, Self::Surface];
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub(crate) enum StreamScope {
    Local,
    PeerRelayed { peer_instance_id: String },
}

#[derive(Default)]
struct Counters {
    live: AtomicU64,
    opened: AtomicU64,
    closed: AtomicU64,
}

pub(crate) struct StreamCensus {
    local: [Arc<Counters>; 4],
    peers: RwLock<BTreeMap<(StreamKind, String), Arc<Counters>>>,
}

impl Default for StreamCensus {
    fn default() -> Self {
        Self {
            local: std::array::from_fn(|_| Arc::new(Counters::default())),
            peers: RwLock::default(),
        }
    }
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub(crate) struct Counts {
    live: u64,
    opened: u64,
    closed: u64,
}

#[derive(Serialize)]
pub(crate) struct StreamEntry {
    kind: StreamKind,
    #[serde(flatten)]
    scope: StreamScope,
    #[serde(flatten)]
    counts: Counts,
}

#[derive(Serialize)]
pub(crate) struct StreamSnapshot {
    #[serde(flatten)]
    totals: Counts,
    entries: Vec<StreamEntry>,
}

impl StreamCensus {
    pub(crate) fn open(&self, kind: StreamKind, scope: StreamScope) -> StreamGuard {
        let counters = match scope {
            StreamScope::Local => self.local[kind as usize].clone(),
            StreamScope::PeerRelayed { peer_instance_id } => {
                let key = (kind, peer_instance_id);
                let existing = self.peers.read().get(&key).cloned();
                existing.unwrap_or_else(|| self.peers.write().entry(key).or_default().clone())
            }
        };
        counters.opened.fetch_add(1, Ordering::Relaxed);
        counters.live.fetch_add(1, Ordering::Relaxed);
        StreamGuard {
            counters,
            diagnostics: None,
        }
    }

    pub(crate) fn snapshot(&self) -> StreamSnapshot {
        let mut entries = StreamKind::ALL
            .into_iter()
            .map(|kind| StreamEntry {
                kind,
                scope: StreamScope::Local,
                counts: self.local[kind as usize].snapshot(),
            })
            .collect::<Vec<_>>();
        entries.extend(
            self.peers
                .read()
                .iter()
                .map(|((kind, peer_instance_id), counters)| StreamEntry {
                    kind: *kind,
                    scope: StreamScope::PeerRelayed {
                        peer_instance_id: peer_instance_id.clone(),
                    },
                    counts: counters.snapshot(),
                }),
        );
        let mut totals = Counts::default();
        for entry in &entries {
            totals.live += entry.counts.live;
            totals.opened += entry.counts.opened;
            totals.closed += entry.counts.closed;
        }
        StreamSnapshot { totals, entries }
    }
}

impl Counters {
    fn snapshot(&self) -> Counts {
        Counts {
            live: self.live.load(Ordering::Relaxed),
            opened: self.opened.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
        }
    }
}

pub(crate) struct StreamGuard {
    counters: Arc<Counters>,
    pub(crate) diagnostics: Option<StreamDiagnostics>,
}

impl StreamGuard {
    pub(crate) fn local(kind: StreamKind) -> Self {
        STREAMS.open(kind, StreamScope::Local)
    }

    pub(crate) fn relayed(kind: StreamKind, peer_instance_id: String) -> Self {
        STREAMS.open(kind, StreamScope::PeerRelayed { peer_instance_id })
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.counters.live.fetch_sub(1, Ordering::Relaxed);
        self.counters.closed.fetch_add(1, Ordering::Relaxed);
        if let Some(diagnostics) = self.diagnostics.as_mut() {
            diagnostics.finish("cancelled");
        }
    }
}

pub(crate) fn track<S: Stream>(stream: S, guard: StreamGuard) -> impl Stream<Item = S::Item> {
    async_stream::stream! {
        let _guard = guard;
        let mut stream = Box::pin(stream);
        while let Some(item) = stream.next().await {
            yield item;
        }
    }
}

#[cfg(test)]
mod tests;
