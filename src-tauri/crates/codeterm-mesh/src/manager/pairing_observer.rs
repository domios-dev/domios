use parking_lot::Mutex;
use std::{collections::HashSet, sync::Arc};

pub struct PairingObserver {
    committed: Mutex<HashSet<String>>,
    emit: Arc<dyn Fn() + Send + Sync>,
}
impl PairingObserver {
    pub fn new(peers: HashSet<String>, emit: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { committed: Mutex::new(peers), emit }
    }
    pub fn committed(&self, peers: HashSet<String>) {
        let count = {
            let mut committed = self.committed.lock();
            let count = peers.difference(&committed).count();
            *committed = peers;
            count
        };
        for _ in 0..count { (self.emit)(); }
    }
}
