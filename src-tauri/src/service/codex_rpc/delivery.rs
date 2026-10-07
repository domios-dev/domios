use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Ready,
    Plain,
}

struct Launch {
    nonce: String,
    connection: Option<String>,
    bootstrap_consumed: bool,
    state: DeliveryState,
}

static LAUNCHES: LazyLock<Mutex<HashMap<String, Launch>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn state(tab: &str) -> DeliveryState {
    LAUNCHES
        .lock()
        .get(tab)
        .map_or(DeliveryState::Plain, |launch| launch.state)
}

pub fn begin(tab: &str, nonce: &str) {
    LAUNCHES.lock().insert(
        tab.into(),
        Launch {
            nonce: nonce.into(),
            connection: None,
            bootstrap_consumed: false,
            state: DeliveryState::Pending,
        },
    );
}

pub fn attached(svc: &ServiceContext, tab: &str, nonce: &str, connection: &str) {
    update(svc, tab, nonce, |launch| {
        launch.connection = Some(connection.into())
    });
}

pub fn detached(tab: &str, nonce: &str, connection: &str) {
    let mut launches = LAUNCHES.lock();
    if let Some(launch) = launches
        .get_mut(tab)
        .filter(|launch| launch.nonce == nonce && launch.connection.as_deref() == Some(connection))
    {
        launch.connection = None;
        launch.state = DeliveryState::Pending;
    }
}

pub fn startup_complete(svc: &ServiceContext, tab: &str, nonce: &str) {
    if super::lifecycle::for_tab(tab).is_none() {
        plain_ready(svc, tab, nonce);
    } else {
        update(svc, tab, nonce, |launch| launch.bootstrap_consumed = true);
    }
}

pub fn plain_launch(tab: &str, nonce: &str) {
    if let Some(launch) = LAUNCHES.lock().get_mut(tab) {
        launch.nonce = nonce.into();
        launch.state = DeliveryState::Pending;
        launch.connection = None;
    }
}

pub fn disconnected(tab: &str) {
    if let Some(launch) = LAUNCHES.lock().get_mut(tab) {
        if launch.state != DeliveryState::Plain {
            launch.state = DeliveryState::Pending;
        }
    }
}

pub fn refresh(svc: &ServiceContext, tab: &str) {
    let nonce = LAUNCHES.lock().get(tab).map(|launch| launch.nonce.clone());
    if let Some(nonce) = nonce {
        update(svc, tab, &nonce, |_| {});
    }
}

pub fn plain_ready(svc: &ServiceContext, tab: &str, nonce: &str) {
    let changed = {
        let mut launches = LAUNCHES.lock();
        let Some(launch) = launches.get_mut(tab).filter(|launch| launch.nonce == nonce) else {
            return;
        };
        let changed = launch.state != DeliveryState::Plain;
        launch.state = DeliveryState::Plain;
        changed
    };
    if changed {
        wake(svc, tab);
    }
}

fn ready(attached: bool, bootstrap_consumed: bool, verified: bool) -> bool {
    attached && bootstrap_consumed && verified
}

fn update(svc: &ServiceContext, tab: &str, nonce: &str, change: impl FnOnce(&mut Launch)) {
    let verified = super::lifecycle::for_tab(tab)
        .is_some_and(|sidecar| sidecar.verified.load(std::sync::atomic::Ordering::Acquire));
    let changed = {
        let mut launches = LAUNCHES.lock();
        let Some(launch) = launches.get_mut(tab).filter(|launch| launch.nonce == nonce) else {
            return;
        };
        change(launch);
        let next = if ready(
            launch.connection.is_some(),
            launch.bootstrap_consumed,
            verified,
        ) {
            DeliveryState::Ready
        } else {
            DeliveryState::Pending
        };
        let changed = launch.state != next && next == DeliveryState::Ready;
        launch.state = next;
        changed
    };
    if changed {
        wake(svc, tab);
    }
}

fn wake(svc: &ServiceContext, tab: &str) {
    svc.wake_parked_user_send(tab);
    svc.wake_agent_deliveries_on_state_change(tab);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_attach_delivery_requires_attachment_bootstrap_and_binding() {
        for attached in [false, true] {
            for consumed in [false, true] {
                for verified in [false, true] {
                    assert_eq!(
                        ready(attached, consumed, verified),
                        attached && consumed && verified
                    );
                }
            }
        }
    }
}
