use super::AnnounceReport;
use crate::{account_mesh::BoxFuture, MeshState};
use parking_lot::Mutex;

#[derive(Default)]
struct Pending {
    running: bool,
    requested: bool,
    delivered: Option<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct AddressAnnouncements(Mutex<Pending>);

struct Owner<'a> {
    queue: &'a AddressAnnouncements,
    active: bool,
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        if self.active {
            self.queue.0.lock().running = false;
        }
    }
}

pub(super) async fn publish_with(
    mesh: &MeshState,
    send: impl for<'a> Fn(&'a MeshState, crate::types::PeerAnnounce) -> BoxFuture<'a, AnnounceReport>,
) -> AnnounceReport {
    let queue = mesh.lock().address_announcements.clone();
    {
        let mut pending = queue.0.lock();
        pending.requested = true;
        if pending.running {
            return AnnounceReport::default();
        }
        pending.running = true;
    }
    let mut owner = Owner {
        queue: &queue,
        active: true,
    };
    let mut total = AnnounceReport::default();
    loop {
        tokio::task::yield_now().await;
        {
            let mut pending = queue.0.lock();
            if !pending.requested {
                pending.running = false;
                owner.active = false;
                return total;
            }
            pending.requested = false;
        }
        let snapshot = {
            let manager = mesh.lock();
            let announce = manager.build_announce();
            serde_json::to_vec(&announce).map(|key| (announce, key))
        };
        let Ok((announce, snapshot)) = snapshot else {
            continue;
        };
        if queue.0.lock().delivered.as_ref() == Some(&snapshot) {
            continue;
        }
        let report = send(mesh, announce).await;
        queue.0.lock().delivered = Some(snapshot);
        total.sent += report.sent;
        total.ok += report.ok;
        total.stale_peers.extend(report.stale_peers);
        total.errors.extend(report.errors);
    }
}

#[cfg(test)]
mod tests;
