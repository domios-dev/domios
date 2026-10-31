use parking_lot::Mutex;
use std::{collections::HashSet, sync::Arc};

pub struct ConnectionObserver {
    committed: Mutex<HashSet<String>>,
    emit: Arc<dyn Fn(usize) + Send + Sync>,
    counts: Arc<dyn Fn(usize) + Send + Sync>,
}
impl ConnectionObserver {
    pub fn new(
        peers: HashSet<String>,
        emit: Arc<dyn Fn(usize) + Send + Sync>,
        counts: Arc<dyn Fn(usize) + Send + Sync>,
    ) -> Self {
        Self {
            committed: Mutex::new(peers),
            emit,
            counts,
        }
    }
    pub fn committed(&self, peers: HashSet<String>) {
        let machines = peers.len().saturating_add(1);
        let count = {
            let mut committed = self.committed.lock();
            let count = peers.difference(&committed).count();
            *committed = peers;
            count
        };
        (self.counts)(machines);
        for _ in 0..count {
            (self.emit)(machines);
        }
    }
}
