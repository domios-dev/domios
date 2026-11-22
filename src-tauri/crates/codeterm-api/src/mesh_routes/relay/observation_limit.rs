use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, SemaphorePermit};

/// Cap concurrent inbound peer observation relays so polling/event storms from
/// remote peers cannot exhaust local loopback sockets or `rest-api-worker`.
const GLOBAL_PEER_OBSERVATIONS: usize = 16;
const PER_PEER_OBSERVATIONS: usize = 4;

struct PeerObservationLimiter {
    global: Semaphore,
    per_peer: Mutex<HashMap<String, Arc<Semaphore>>>,
}

pub(super) struct PeerObservationPermit {
    _global: SemaphorePermit<'static>,
    _peer: OwnedSemaphorePermit,
}

impl PeerObservationLimiter {
    fn new() -> Self {
        Self {
            global: Semaphore::new(GLOBAL_PEER_OBSERVATIONS),
            per_peer: Mutex::new(HashMap::new()),
        }
    }

    fn peer_semaphore(&self, peer_id: &str) -> Arc<Semaphore> {
        let mut peers = self.per_peer.lock();
        if peers.len() > 64 {
            peers.retain(|_, sem| Arc::strong_count(sem) > 1);
        }
        peers
            .entry(peer_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_PEER_OBSERVATIONS)))
            .clone()
    }
}

static LIMITER: LazyLock<PeerObservationLimiter> = LazyLock::new(PeerObservationLimiter::new);

pub(super) async fn acquire_peer_observation(peer_id: &str) -> Option<PeerObservationPermit> {
    // Acquire per-peer first so one noisy peer cannot hold global slots while waiting on itself.
    let peer = LIMITER.peer_semaphore(peer_id).acquire_owned().await.ok()?;
    let global = LIMITER.global.acquire().await.ok()?;
    Some(PeerObservationPermit {
        _global: global,
        _peer: peer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test]
    async fn per_peer_observation_limit_caps_single_peer_and_releases_on_drop() {
        let peer = "peer-storm-unit-test";
        let mut held = Vec::new();
        for _ in 0..PER_PEER_OBSERVATIONS {
            held.push(acquire_peer_observation(peer).await.unwrap());
        }
        let mut fifth = Box::pin(acquire_peer_observation(peer));
        assert!(
            fifth.as_mut().now_or_never().is_none(),
            "5th concurrent observation from the same peer must park asynchronously"
        );
        // Dropping the 5th waiter cancels cleanly without blocking.
        drop(fifth);
        // A different peer can still acquire its own slots while the first peer is saturated.
        let other = acquire_peer_observation("peer-other-unit-test")
            .now_or_never()
            .expect("other peer must not be blocked by a single noisy peer")
            .unwrap();
        drop(other);
        held.pop();
        let next = acquire_peer_observation(peer)
            .now_or_never()
            .expect("releasing one permit must admit the next observation")
            .unwrap();
        drop(next);
        drop(held);
    }
}
