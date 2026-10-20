use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Weak};

use codeterm_util::lock_hold::TimedMutex;

use super::session_window::SessionSnapshot;

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct SnapshotKey {
    pub tab_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub binding_nonce: Option<String>,
    pub incarnation: Option<String>,
    pub path: PathBuf,
}

type SnapshotResult = Result<Arc<SessionSnapshot>, String>;

struct FlightState {
    result: Option<SnapshotResult>,
    waiters: Vec<mpsc::SyncSender<SnapshotResult>>,
}

struct Flight {
    state: TimedMutex<FlightState>,
}

impl Flight {
    fn new() -> Self {
        Self {
            state: TimedMutex::new(
                "chat_snapshot_result",
                FlightState {
                    result: None,
                    waiters: vec![],
                },
            ),
        }
    }

    fn publish(&self, result: SnapshotResult) {
        let waiters = {
            let mut state = self.state.lock();
            state.result = Some(result.clone());
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
    }

    fn wait(&self) -> SnapshotResult {
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut state = self.state.lock();
            if let Some(result) = &state.result {
                return result.clone();
            }
            state.waiters.push(sender);
        }
        receiver
            .recv()
            .map_err(|_| "Chat snapshot producer disconnected".to_string())?
    }
}

struct Producer {
    flight: Arc<Flight>,
    completed: bool,
}

impl Producer {
    fn finish(mut self, snapshot: SessionSnapshot) -> Arc<SessionSnapshot> {
        let snapshot = Arc::new(snapshot);
        self.flight.publish(Ok(snapshot.clone()));
        self.completed = true;
        snapshot
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        if !self.completed {
            self.flight
                .publish(Err("Chat snapshot producer aborted".into()));
        }
    }
}

pub struct SnapshotFlights {
    active: TimedMutex<HashMap<SnapshotKey, Weak<Flight>>>,
}

impl Default for SnapshotFlights {
    fn default() -> Self {
        Self {
            active: TimedMutex::new("chat_snapshot_flights", HashMap::new()),
        }
    }
}

impl SnapshotFlights {
    fn acquire(&self, key: SnapshotKey) -> (Arc<Flight>, bool) {
        let mut active = self.active.lock();
        active.retain(|_, flight| flight.strong_count() > 0);
        if let Some(flight) = active.get(&key).and_then(Weak::upgrade) {
            return (flight, false);
        }
        let flight = Arc::new(Flight::new());
        active.insert(key, Arc::downgrade(&flight));
        (flight, true)
    }

    pub fn run(&self, key: SnapshotKey, parse: impl FnOnce() -> SessionSnapshot) -> SnapshotResult {
        let (flight, leader) = self.acquire(key);
        if leader {
            let producer = Producer {
                flight,
                completed: false,
            };
            Ok(producer.finish(parse()))
        } else {
            flight.wait()
        }
    }
}

#[cfg(test)]
mod tests;
