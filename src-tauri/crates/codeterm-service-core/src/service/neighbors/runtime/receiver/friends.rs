use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct Cell {
    known: parking_lot::Mutex<BTreeMap<String, KnownReceiver>>,
    version: AtomicU64,
}

struct KnownReceiver {
    generation: u64,
    receiver: FriendReceiver,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::service::neighbors) struct FriendReceiver {
    pub status: ReceiverStatus,
    pub binding: String,
}

impl FriendReceiver {
    #[cfg(test)]
    pub fn from_status(status: &ReceiverStatus) -> Self {
        Self {
            status: status.clone(),
            binding: codeterm_neighbors::crypto::sha256_hex(&serde_json::to_vec(status).unwrap()),
        }
    }

    pub fn from_projection(
        receiver: codeterm_neighbors::wire::EffectiveReceiver,
        installation: &str,
    ) -> Result<Self, String> {
        if receiver.revision > codeterm_neighbors::wire::MAX_SAFE_INTEGER
            || receiver.receiving
                && (receiver.installation_id.is_none() || receiver.enc_key_id.is_none())
        {
            return Err(Error::InvalidEnvelope.to_string());
        }
        let fields = json!([
            receiver.installation_id,
            receiver.enc_key_id,
            receiver.revision,
            receiver.receiving
        ]);
        let binding = codeterm_neighbors::crypto::sha256_hex(
            &serde_json::to_vec(&fields).map_err(|_| Error::InvalidEnvelope.to_string())?,
        );
        Ok(Self {
            status: ReceiverStatus::effective(&receiver, installation),
            binding,
        })
    }
}

pub(in crate::service::neighbors) struct Change {
    pub version: u64,
    pub friendship_id: String,
    pub previous: Option<FriendReceiver>,
    pub current: Option<FriendReceiver>,
}

impl ServiceContext {
    pub(in crate::service::neighbors) fn neighbor_friend_receiver_views(&self) -> Value {
        self.neighbor_friend_receiver_snapshot().0
    }

    pub(in crate::service::neighbors) fn neighbor_friend_receiver_snapshot(&self) -> (Value, u64) {
        let generation = self.neighbor_cell().generation.load(Ordering::Acquire);
        let cell = self.extensions.get_or_init(Cell::default);
        let known = cell.known.lock();
        let views: serde_json::Map<String, Value> = known
            .iter()
            .filter(|(_, known)| known.generation == generation)
            .map(|(id, known)| {
                let view = Self::receiver_view(&known.receiver.status);
                (id.clone(), view)
            })
            .collect();
        (Value::Object(views), cell.version.load(Ordering::Acquire))
    }

    pub(in crate::service::neighbors) fn stop_neighbor_concierge_listener(&self, friendship: &str) {
        let stopped = {
            let mut listeners = self.neighbor_cell().listeners.lock();
            if let Some(listener) = listeners.active.remove(friendship) {
                let _ = listener.shutdown.send(true);
                listeners.retired.insert(friendship.into(), listener.task);
                true
            } else {
                false
            }
        };
        if stopped {
            self.neighbor_cell().states.lock().insert(
                friendship.into(),
                codeterm_neighbors::supervisor::ListenerState::Stopped,
            );
            self.ui.emit(
                "neighbor-runtime-status",
                json!({"friendship_id":friendship,"status":"stopped"}),
            );
        }
    }
    pub(in crate::service::neighbors) fn neighbor_friend_receiving_here(
        &self,
        friendship: &str,
    ) -> bool {
        self.neighbor_friend_receiver(friendship)
            .is_some_and(|status| status.receiving_here())
    }

    pub(in crate::service::neighbors) fn neighbor_friend_receiver(
        &self,
        friendship: &str,
    ) -> Option<ReceiverStatus> {
        self.require_neighbors_enabled().ok()?;
        let generation = self.neighbor_cell().generation.load(Ordering::Acquire);
        if self.neighbor_stop_local_pending(generation) {
            return None;
        }
        self.extensions
            .get_or_init(Cell::default)
            .known
            .lock()
            .get(friendship)
            .filter(|known| known.generation == generation)
            .map(|known| known.receiver.status.clone())
    }

    pub(in crate::service::neighbors) fn neighbor_friend_receiver_binding(
        &self,
        friendship: &str,
    ) -> Option<String> {
        self.require_neighbors_enabled().ok()?;
        let generation = self.neighbor_cell().generation.load(Ordering::Acquire);
        if self.neighbor_stop_local_pending(generation) {
            return None;
        }
        self.extensions
            .get_or_init(Cell::default)
            .known
            .lock()
            .get(friendship)
            .filter(|known| known.generation == generation)
            .map(|known| known.receiver.binding.clone())
    }

    pub(in crate::service::neighbors) fn require_neighbor_friend_receiver(
        &self,
        friendship: &str,
        revision: Option<u64>,
    ) -> codeterm_neighbors::Result<()> {
        self.neighbor_friend_receiver(friendship)
            .filter(|status| {
                status.receiving_here()
                    && revision.is_none_or(|revision| revision == status.receiver_revision)
            })
            .map(|_| ())
            .ok_or(Error::NotReceiver)
    }

    pub(in crate::service::neighbors) fn forget_neighbor_friend_receivers(&self) -> u64 {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut known = cell.known.lock();
        known.clear();
        cell.version.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(in crate::service::neighbors) fn forget_neighbor_friend_receiver(
        &self,
        friendship: &str,
    ) -> u64 {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut known = cell.known.lock();
        known.remove(friendship);
        cell.version.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(in crate::service::neighbors) fn remember_neighbor_friend_receiver(
        &self,
        friendship: &str,
        receiver: FriendReceiver,
        generation: u64,
    ) -> Result<Vec<Change>, String> {
        self.require_neighbor_generation(generation)?;
        let cell = self.extensions.get_or_init(Cell::default);
        let mut known = cell.known.lock();
        let previous = known
            .get(friendship)
            .filter(|known| known.generation == generation)
            .map(|known| known.receiver.clone());
        if previous.as_ref() == Some(&receiver) {
            return Ok(vec![]);
        }
        known.insert(
            friendship.into(),
            KnownReceiver {
                generation,
                receiver: receiver.clone(),
            },
        );
        let version = cell.version.fetch_add(1, Ordering::AcqRel) + 1;
        Ok(vec![Change {
            version,
            friendship_id: friendship.into(),
            previous,
            current: Some(receiver),
        }])
    }

    pub(in crate::service::neighbors) fn remember_neighbor_friend_receivers(
        &self,
        receivers: BTreeMap<String, FriendReceiver>,
        generation: u64,
    ) -> Result<Vec<Change>, String> {
        self.require_neighbor_generation(generation)?;
        let cell = self.extensions.get_or_init(Cell::default);
        let mut known = cell.known.lock();
        let mut changes = Vec::new();
        for friendship in known
            .keys()
            .chain(receivers.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let previous = known
                .get(&friendship)
                .filter(|value| value.generation == generation)
                .map(|value| value.receiver.clone());
            let current = receivers.get(&friendship).cloned();
            if previous != current {
                changes.push(Change {
                    version: 0,
                    friendship_id: friendship,
                    previous,
                    current,
                });
            }
        }
        *known = receivers
            .into_iter()
            .map(|(id, receiver)| {
                (
                    id,
                    KnownReceiver {
                        generation,
                        receiver,
                    },
                )
            })
            .collect();
        if !changes.is_empty() {
            let version = cell.version.fetch_add(1, Ordering::AcqRel) + 1;
            for change in &mut changes {
                change.version = version;
            }
        }
        Ok(changes)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_neighbor_friend_receiver_for_test(
        &self,
        friendship: &str,
        receiving_here: Option<bool>,
        revision: u64,
    ) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut known = cell.known.lock();
        if let Some(here) = receiving_here {
            known.insert(
                friendship.into(),
                KnownReceiver {
                    generation: self.neighbor_cell().generation.load(Ordering::Acquire),
                    receiver: FriendReceiver {
                        binding: format!("fixture:{here}:{revision}"),
                        status: ReceiverStatus {
                            receiver_revision: revision,
                            receiver_installation_id: Some(
                                if here {
                                    "fixture-here"
                                } else {
                                    "fixture-elsewhere"
                                }
                                .into(),
                            ),
                            receiver_encryption_key_id: None,
                            receiver_machine_label: Some("Fixture".into()),
                            receiving_enabled: true,
                            is_me: here,
                        },
                    },
                },
            );
        } else {
            known.remove(friendship);
        }
        cell.version.fetch_add(1, Ordering::AcqRel);
    }

    #[cfg(test)]
    pub(crate) fn set_neighbor_friend_receiver_binding_for_test(
        &self,
        friendship: &str,
        binding: &str,
    ) {
        let cell = self.extensions.get_or_init(Cell::default);
        if let Some(known) = cell.known.lock().get_mut(friendship) {
            known.receiver.binding = binding.into();
            cell.version.fetch_add(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests;
