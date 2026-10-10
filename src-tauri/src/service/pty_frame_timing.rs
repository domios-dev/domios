use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static SCOPES: LazyLock<Mutex<HashMap<String, Arc<Operation>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Operation {
    id: u64,
    tab: String,
    next_read: AtomicU64,
}

pub(super) struct Scope {
    pty: String,
    operation: Arc<Operation>,
}

impl Scope {
    pub(super) fn begin(pty: &str, tab: &str) -> Self {
        let operation = Arc::new(Operation {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            tab: tab.into(),
            next_read: AtomicU64::new(1),
        });
        let mut scopes = SCOPES.lock();
        scopes.insert(pty.into(), operation.clone());
        ACTIVE.store(scopes.len() as u64, Ordering::Relaxed);
        log::info!(
            "[agent-model/frame] tab={tab} operation={} phase=begin at_ms={}",
            operation.id,
            unix_ms()
        );
        Self {
            pty: pty.into(),
            operation,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let mut scopes = SCOPES.lock();
        if scopes
            .get(&self.pty)
            .is_some_and(|current| Arc::ptr_eq(current, &self.operation))
        {
            scopes.remove(&self.pty);
            ACTIVE.store(scopes.len() as u64, Ordering::Relaxed);
        }
    }
}

pub(super) struct Receipt {
    operation: Arc<Operation>,
    sequence: u64,
    read: Instant,
}

impl Receipt {
    pub(super) fn read_completed(pty: &str, bytes: usize) -> Option<Self> {
        let read = Instant::now();
        if ACTIVE.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let operation = SCOPES.lock().get(pty).cloned()?;
        let sequence = operation.next_read.fetch_add(1, Ordering::Relaxed);
        let receipt = Self {
            operation,
            sequence,
            read,
        };
        receipt.phase("os_read_completed", bytes);
        Some(receipt)
    }

    pub(super) fn phase(&self, phase: &str, bytes: usize) {
        log::info!("[agent-model/frame] tab={} operation={} read={} phase={phase} at_ms={} since_read_us={} bytes={bytes}",
            self.operation.tab, self.operation.id, self.sequence, unix_ms(), self.read.elapsed().as_micros());
    }
}

pub(super) fn parsed(pty: &str, bytes: usize, accepted: bool) {
    if ACTIVE.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(operation) = SCOPES.lock().get(pty) {
        log::info!("[agent-model/frame] tab={} operation={} phase=parse_signal_completed at_ms={} bytes={bytes} accepted={accepted}",
            operation.tab, operation.id, unix_ms());
    }
}

pub(super) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_is_scoped_to_owned_pty_and_cleanup_preserves_replacement() {
        let first = Scope::begin("timing-test-pty", "tab");
        assert!(Receipt::read_completed("foreign-pty", 5).is_none());
        let receipt = Receipt::read_completed("timing-test-pty", 5).unwrap();
        assert_eq!(receipt.sequence, 1);
        let replacement = Scope::begin("timing-test-pty", "new-tab");
        drop(first);
        let next = Receipt::read_completed("timing-test-pty", 7).unwrap();
        assert_eq!(next.operation.id, replacement.operation.id);
        drop(replacement);
        assert!(Receipt::read_completed("timing-test-pty", 5).is_none());
    }
}
