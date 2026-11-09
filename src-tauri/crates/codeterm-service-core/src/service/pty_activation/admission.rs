use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) const MAX_CONTENDERS: usize = 32;
const MAX_TABS: usize = 4096;

struct TabGate {
    admission: Arc<Semaphore>,
    owner: Arc<Semaphore>,
}

fn gates() -> &'static Mutex<HashMap<String, Weak<TabGate>>> {
    static GATES: OnceLock<Mutex<HashMap<String, Weak<TabGate>>>> = OnceLock::new();
    GATES.get_or_init(Mutex::default)
}

pub(super) struct Admission {
    gate: Arc<TabGate>,
    _slot: OwnedSemaphorePermit,
}

impl Admission {
    pub(super) fn new(tab_id: &str) -> Result<Self, String> {
        let gate = {
            let mut gates = gates().lock();
            if let Some(gate) = gates.get(tab_id).and_then(Weak::upgrade) {
                gate
            } else {
                gates.retain(|_, gate| gate.strong_count() != 0);
                if gates.len() >= MAX_TABS {
                    return Err("tab_busy_refused: activation tab capacity reached".into());
                }
                let gate = Arc::new(TabGate {
                    admission: Arc::new(Semaphore::new(MAX_CONTENDERS)),
                    owner: Arc::new(Semaphore::new(1)),
                });
                gates.insert(tab_id.to_string(), Arc::downgrade(&gate));
                gate
            }
        };
        let slot =
            gate.admission.clone().try_acquire_owned().map_err(|_| {
                "tab_busy_refused: activation contender capacity reached".to_string()
            })?;
        Ok(Self { gate, _slot: slot })
    }

    pub(super) async fn acquire(
        self,
        mut waiting: impl FnMut(),
    ) -> Result<super::TabActivationPermit, String> {
        let owner = match self.gate.owner.clone().try_acquire_owned() {
            Ok(owner) => owner,
            Err(_) => {
                waiting();
                self.gate
                    .owner
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| "tab_busy_refused: activation closed".to_string())?
            }
        };
        Ok(super::TabActivationPermit {
            _owner: owner,
            _admission: self,
        })
    }
}
