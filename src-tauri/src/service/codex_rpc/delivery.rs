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
    attach_failed: bool,
    state: DeliveryState,
}

impl Launch {
    fn new(nonce: &str) -> Self {
        Self {
            nonce: nonce.into(),
            connection: None,
            bootstrap_consumed: false,
            attach_failed: false,
            state: DeliveryState::Pending,
        }
    }

    fn advance(&mut self, verified: bool) -> bool {
        let next = if self.connection.is_some()
            && self.bootstrap_consumed
            && verified
            && !self.attach_failed
        {
            DeliveryState::Ready
        } else {
            DeliveryState::Pending
        };
        let changed = self.state != next && next == DeliveryState::Ready;
        self.state = next;
        changed
    }

    fn disconnect(&mut self) {
        self.connection = None;
        self.state = DeliveryState::Pending;
        self.attach_failed = true;
    }
}

static LAUNCHES: LazyLock<Mutex<HashMap<String, Launch>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CHANGED: parking_lot::Condvar = parking_lot::Condvar::new();

pub fn wait_attached(tab: &str, nonce: &str, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    let mut launches = LAUNCHES.lock();
    loop {
        let Some(launch) = launches.get(tab).filter(|launch| launch.nonce == nonce) else {
            return false;
        };
        if launch.attach_failed {
            return false;
        }
        if launch.connection.is_some() {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        CHANGED.wait_for(&mut launches, deadline - now);
    }
}

pub fn state(tab: &str) -> DeliveryState {
    LAUNCHES
        .lock()
        .get(tab)
        .map_or(DeliveryState::Plain, |launch| launch.state)
}

pub fn begin(tab: &str, nonce: &str) {
    LAUNCHES.lock().insert(tab.into(), Launch::new(nonce));
    CHANGED.notify_all();
}

pub fn attached(svc: &ServiceContext, tab: &str, nonce: &str, connection: &str) {
    update(svc, tab, nonce, |launch| {
        launch.connection = Some(connection.into());
        launch.attach_failed = false;
    });
}

pub fn detached(tab: &str, nonce: &str, connection: &str) {
    let mut launches = LAUNCHES.lock();
    if let Some(launch) = launches
        .get_mut(tab)
        .filter(|launch| launch.nonce == nonce && launch.connection.as_deref() == Some(connection))
    {
        launch.disconnect();
        CHANGED.notify_all();
    }
}

pub fn attach_failed(tab: &str, nonce: &str) {
    if let Some(launch) = LAUNCHES
        .lock()
        .get_mut(tab)
        .filter(|launch| launch.nonce == nonce && launch.connection.is_none())
    {
        launch.attach_failed = true;
        CHANGED.notify_all();
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
            launch.disconnect();
            CHANGED.notify_all();
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
        super::recovery::completed(tab);
        wake(svc, tab);
    }
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
        let changed = launch.advance(verified);
        CHANGED.notify_all();
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
    fn codex_rpc_attach_delivery_waits_for_both_latches_and_wakes_once() {
        let mut launch = Launch::new("own");
        assert!(!launch.advance(true));
        launch.connection = Some("remote".into());
        assert!(!launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Pending);
        launch.bootstrap_consumed = true;
        assert!(!launch.advance(false));
        assert!(launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Ready);
        assert!(!launch.advance(true));
    }

    #[test]
    fn codex_rpc_attach_failure_keeps_pending_after_endpoint_clear() {
        let mut launch = Launch::new("own");
        launch.bootstrap_consumed = true;
        launch.disconnect();
        assert!(!launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Pending);
    }
}
