use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Ready,
    Plain,
}

struct Launch {
    nonce: String,
    connections: HashSet<String>,
    bootstrap_consumed: bool,
    attach_failed: bool,
    state: DeliveryState,
}

impl Launch {
    fn new(nonce: &str) -> Self {
        Self {
            nonce: nonce.into(),
            connections: HashSet::new(),
            bootstrap_consumed: false,
            attach_failed: false,
            state: DeliveryState::Pending,
        }
    }

    fn select_plain(&mut self, nonce: &str) -> bool {
        let changed = self.state != DeliveryState::Plain;
        self.nonce = nonce.into();
        self.state = DeliveryState::Plain;
        self.connections.clear();
        self.attach_failed = false;
        changed
    }

    fn advance(&mut self, verified: bool) -> bool {
        if self.state == DeliveryState::Plain {
            return false;
        }
        let next = if !self.connections.is_empty()
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

    fn attach(&mut self, connection: &str) {
        self.connections.insert(connection.into());
        self.attach_failed = false;
    }

    fn detach(&mut self, connection: &str) -> bool {
        if !self.connections.remove(connection) || !self.connections.is_empty() {
            return false;
        }
        self.state = DeliveryState::Pending;
        self.attach_failed = false;
        true
    }

    fn prepare_retry(&mut self) -> bool {
        if !self.connections.is_empty() {
            return true;
        }
        self.attach_failed = false;
        false
    }

    fn fail_attachment(&mut self) {
        if self.connections.is_empty() {
            self.attach_failed = true;
        }
    }

    fn disconnect(&mut self) {
        self.connections.clear();
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
        if !launch.connections.is_empty() {
            return true;
        }
        if launch.attach_failed {
            return false;
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

pub fn attached_for_launch(tab: &str, nonce: &str) -> bool {
    LAUNCHES.lock().get(tab).is_some_and(|launch| {
        launch.nonce == nonce && !launch.connections.is_empty() && !launch.attach_failed
    })
}

pub fn attached_count(tab: &str, nonce: &str) -> usize {
    LAUNCHES
        .lock()
        .get(tab)
        .filter(|launch| launch.nonce == nonce)
        .map_or(0, |launch| launch.connections.len())
}

pub fn begin(tab: &str, nonce: &str) {
    LAUNCHES.lock().insert(tab.into(), Launch::new(nonce));
    CHANGED.notify_all();
}

pub fn attached(svc: &ServiceContext, tab: &str, nonce: &str, connection: &str) {
    update(svc, tab, nonce, |launch| {
        launch.attach(connection);
    });
    if attached_for_launch(tab, nonce) {
        super::recovery::attachment_observed(tab, nonce);
    }
}

pub fn detached(tab: &str, nonce: &str, connection: &str) -> bool {
    let mut launches = LAUNCHES.lock();
    let Some(launch) = launches.get_mut(tab).filter(|launch| launch.nonce == nonce) else {
        return false;
    };
    let last = launch.detach(connection);
    CHANGED.notify_all();
    last
}

pub fn retry_attachment(tab: &str, nonce: &str) -> bool {
    let mut launches = LAUNCHES.lock();
    let Some(launch) = launches.get_mut(tab).filter(|launch| launch.nonce == nonce) else {
        return false;
    };
    let attached = launch.prepare_retry();
    CHANGED.notify_all();
    attached
}

pub fn attach_failed(tab: &str, nonce: &str) {
    if let Some(launch) = LAUNCHES
        .lock()
        .get_mut(tab)
        .filter(|launch| launch.nonce == nonce && launch.connections.is_empty())
    {
        launch.fail_attachment();
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

pub fn plain_launch(svc: &ServiceContext, tab: &str, nonce: &str) {
    if !svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.session_binding_nonce.as_deref() == Some(nonce))
    {
        return;
    }
    let changed = LAUNCHES
        .lock()
        .get_mut(tab)
        .is_some_and(|launch| launch.select_plain(nonce));
    CHANGED.notify_all();
    if changed {
        wake(svc, tab);
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
    fn codex_rpc_send_plain_fallback_is_terminal_without_rpc_attach_or_bootstrap() {
        let mut launch = Launch::new("starting");
        assert_eq!(launch.state, DeliveryState::Pending);
        assert!(launch.select_plain("plain-launch"));
        assert_eq!(launch.state, DeliveryState::Plain);
        assert_eq!(launch.nonce, "plain-launch");
        assert!(!launch.bootstrap_consumed);
        assert!(launch.connections.is_empty());
        assert!(!launch.advance(false));
        assert!(!launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Plain);
        assert!(!launch.select_plain("plain-launch"));
    }

    #[test]
    fn codex_rpc_attach_delivery_waits_for_both_latches_and_wakes_once() {
        let mut launch = Launch::new("own");
        assert!(!launch.advance(true));
        launch.attach("remote");
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
    #[test]
    fn codex_rpc_attach_overlapping_connections_keep_ready_until_last_closes() {
        let mut launch = Launch::new("own");
        launch.bootstrap_consumed = true;
        launch.attach("first");
        assert!(launch.advance(true));
        launch.attach("second");
        assert!(!launch.advance(true));
        assert!(!launch.detach("second"));
        assert_eq!(launch.state, DeliveryState::Ready);
        assert_eq!(launch.connections.len(), 1);
        assert!(launch.detach("first"));
        assert_eq!(launch.state, DeliveryState::Pending);
    }

    #[test]
    fn codex_rpc_attach_short_and_duplicate_connections_cannot_demote_qualified_attachment() {
        let mut launch = Launch::new("own");
        launch.bootstrap_consumed = true;
        launch.attach("qualified");
        launch.advance(true);
        assert!(!launch.detach("authenticated-only"));
        launch.attach("qualified");
        assert_eq!(launch.connections.len(), 1);
        assert!(!launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Ready);
        assert!(launch.detach("qualified"));
        assert!(!launch.detach("qualified"));
    }

    #[test]
    fn codex_rpc_attach_many_overlapping_connections_preserve_existing_authority() {
        let mut launch = Launch::new("own");
        launch.bootstrap_consumed = true;
        for id in 0..128 {
            launch.attach(&id.to_string());
        }
        assert_eq!(launch.connections.len(), 128);
        assert!(launch.advance(true));
        for id in (1..128).rev() {
            assert!(!launch.detach(&id.to_string()));
        }
        assert_eq!(launch.state, DeliveryState::Ready);
        assert!(launch.detach("0"));
        launch.attach("reconnected");
        assert!(launch.advance(true));
    }
    #[test]
    fn codex_rpc_attach_recovery_and_late_failure_preserve_reconnected_live_client() {
        let mut launch = Launch::new("own");
        launch.bootstrap_consumed = true;
        launch.fail_attachment();
        assert!(!launch.advance(true));
        launch.attach("reconnected");
        assert!(launch.advance(true));
        launch.fail_attachment();
        assert!(launch.prepare_retry());
        assert_eq!(launch.connections.len(), 1);
        assert!(!launch.advance(true));
        assert_eq!(launch.state, DeliveryState::Ready);
        launch.detach("reconnected");
        assert!(!launch.prepare_retry());
        assert_eq!(launch.state, DeliveryState::Pending);
    }
}
