use axum::extract::{ConnectInfo, Request};
use futures_util::StreamExt;
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct RequestStreams {
    generation: u64,
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
}

impl Default for RequestStreams {
    fn default() -> Self {
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            counts: Arc::default(),
        }
    }
}

pub(super) struct RequestContext {
    path: String,
    peer: String,
    client: String,
}

impl RequestStreams {
    pub(super) fn accept(&self, request: &Request) -> RequestContext {
        let context = RequestContext {
            path: request.uri().path().to_owned(),
            peer: request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|peer| peer.0.ip().to_string())
                .unwrap_or_else(|| "unknown".into()),
            client: request
                .headers()
                .get("x-codeterm-client-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown")
                .to_owned(),
        };
        log::debug!(
            "[rest-request] accepted method={} path={} peer={} client={} generation={}",
            request.method(),
            context.path,
            context.peer,
            context.client,
            self.generation
        );
        context
    }

    pub(super) fn open(&self, context: RequestContext) -> StreamGuard {
        let count = change_count(&mut self.counts.lock(), &context.peer, true);
        let guard = StreamGuard {
            owner: self.clone(),
            context,
        };
        guard.log("open", count);
        guard
    }
}

fn change_count(counts: &mut BTreeMap<String, usize>, peer: &str, opening: bool) -> usize {
    if opening {
        let count = counts.entry(peer.into()).or_default();
        *count += 1;
        *count
    } else {
        let remaining = counts
            .get(peer)
            .copied()
            .unwrap_or_default()
            .saturating_sub(1);
        if remaining == 0 {
            counts.remove(peer);
        } else {
            counts.insert(peer.into(), remaining);
        }
        remaining
    }
}

pub(super) struct StreamGuard {
    owner: RequestStreams,
    context: RequestContext,
}

impl StreamGuard {
    fn log(&self, event: &str, count: usize) {
        log::debug!(
            "[rest-sse] event={} path={} peer={} client={} generation={} open_count={}",
            event,
            self.context.path,
            self.context.peer,
            self.context.client,
            self.owner.generation,
            count
        );
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let count = change_count(&mut self.owner.counts.lock(), &self.context.peer, false);
        self.log("close", count);
    }
}

pub(super) fn track<S: futures_core::Stream>(
    stream: S,
    guard: StreamGuard,
) -> impl futures_core::Stream<Item = S::Item> {
    async_stream::stream! {
        let _guard = guard;
        tokio::pin!(stream);
        while let Some(item) = stream.next().await { yield item; }
    }
}

#[cfg(test)]
mod tests;
