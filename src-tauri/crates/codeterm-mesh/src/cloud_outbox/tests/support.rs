use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::account_mesh::ports::BoxFuture;
use crate::crown::{AuthenticatedAgent, AuthenticatedOrigin};

use super::super::ports::{
    DrainAuthorizationPolicy, DrainLifecycleObserver, DrainPolicyDecision, OutboxClock,
    ReverseStreamSink,
};
use super::super::types::{
    CallerContext, OutboundOperation, OutboundPeerRequest, OutboxFrame, OutboxId,
    OutboxRecordStatus, StreamDeliveryOutcome,
};

pub struct TestTempDir {
    path: PathBuf,
}

impl TestTempDir {
    pub fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct ManualClock {
    now_ms: AtomicU64,
}

impl ManualClock {
    pub fn new(initial_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            now_ms: AtomicU64::new(initial_ms),
        })
    }

    pub fn advance(&self, delta_ms: u64) {
        self.now_ms.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl OutboxClock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}

pub fn sample_request(
    account_id: &str,
    source_installation_id: &str,
    target_installation_id: &str,
    operation: OutboundOperation,
    body: &[u8],
) -> OutboundPeerRequest {
    OutboundPeerRequest {
        account_id: account_id.to_string(),
        source_installation_id: source_installation_id.to_string(),
        target_installation_id: target_installation_id.to_string(),
        operation,
        caller_context: CallerContext {
            origin: AuthenticatedOrigin::Agent,
            agent: Some(AuthenticatedAgent {
                instance_id: source_installation_id.to_string(),
                tab_id: "tab-worker-1".to_string(),
                can_spawn: false,
            }),
            expires_at_ms: None,
        },
        body: body.to_vec(),
    }
}

pub enum ScriptedAction {
    Immediate(StreamDeliveryOutcome),
    HoldUntilRelease {
        entered: Arc<Notify>,
        release: Arc<Notify>,
        outcome: StreamDeliveryOutcome,
    },
}

pub struct ScriptedSink {
    stream_id: u64,
    account_id: String,
    local_installation_id: String,
    peer_installation_id: String,
    actions: Mutex<VecDeque<ScriptedAction>>,
    default_outcome: StreamDeliveryOutcome,
    delivered: Mutex<Vec<OutboxFrame>>,
    delivered_notify: Arc<Notify>,
}

impl ScriptedSink {
    pub fn new(
        stream_id: u64,
        account_id: &str,
        local_installation_id: &str,
        peer_installation_id: &str,
        actions: Vec<ScriptedAction>,
        default_outcome: StreamDeliveryOutcome,
    ) -> Arc<Self> {
        Arc::new(Self {
            stream_id,
            account_id: account_id.to_string(),
            local_installation_id: local_installation_id.to_string(),
            peer_installation_id: peer_installation_id.to_string(),
            actions: Mutex::new(VecDeque::from(actions)),
            default_outcome,
            delivered: Mutex::new(Vec::new()),
            delivered_notify: Arc::new(Notify::new()),
        })
    }

    pub fn ack_all(
        stream_id: u64,
        account_id: &str,
        local_installation_id: &str,
        peer_installation_id: &str,
    ) -> Arc<Self> {
        Self::new(
            stream_id,
            account_id,
            local_installation_id,
            peer_installation_id,
            Vec::new(),
            StreamDeliveryOutcome::acked(None),
        )
    }

    pub fn delivered_ids(&self) -> Vec<OutboxId> {
        self.delivered
            .lock()
            .iter()
            .map(|frame| frame.id.clone())
            .collect()
    }

    pub async fn wait_for_delivery_count(&self, count: usize) {
        loop {
            let notified = self.delivered_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.delivered.lock().len() >= count {
                return;
            }
            tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .expect("timed out waiting for sink deliveries");
        }
    }
}

impl ReverseStreamSink for ScriptedSink {
    fn stream_id(&self) -> u64 {
        self.stream_id
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn local_installation_id(&self) -> &str {
        &self.local_installation_id
    }

    fn peer_installation_id(&self) -> &str {
        &self.peer_installation_id
    }

    fn deliver<'a>(&'a self, frame: &'a OutboxFrame) -> BoxFuture<'a, StreamDeliveryOutcome> {
        let action = self.actions.lock().pop_front();
        self.delivered.lock().push(frame.clone());
        self.delivered_notify.notify_waiters();
        let fallback = self.default_outcome.clone();
        Box::pin(async move {
            match action {
                Some(ScriptedAction::Immediate(outcome)) => outcome,
                Some(ScriptedAction::HoldUntilRelease {
                    entered,
                    release,
                    outcome,
                }) => {
                    entered.notify_waiters();
                    release.notified().await;
                    outcome
                }
                None => fallback,
            }
        })
    }
}

pub struct SettledWaiter {
    settled: Mutex<HashMap<OutboxId, OutboxRecordStatus>>,
    claimed: Mutex<Vec<(OutboxId, u64)>>,
    notify: Arc<Notify>,
}

impl SettledWaiter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            settled: Mutex::new(HashMap::new()),
            claimed: Mutex::new(Vec::new()),
            notify: Arc::new(Notify::new()),
        })
    }

    pub async fn wait_for_settled(&self, id: &OutboxId) -> OutboxRecordStatus {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(status) = self.settled.lock().get(id).cloned() {
                return status;
            }
            tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .expect("timed out waiting for settled outbox record");
        }
    }

    pub fn claim_count_for(&self, id: &OutboxId) -> usize {
        self.claimed
            .lock()
            .iter()
            .filter(|(claimed_id, _)| claimed_id == id)
            .count()
    }
}

impl DrainLifecycleObserver for SettledWaiter {
    fn on_claimed_persisted(&self, id: &OutboxId, stream_id: u64) {
        self.claimed.lock().push((id.clone(), stream_id));
        self.notify.notify_waiters();
    }

    fn on_settled(&self, id: &OutboxId, status: &OutboxRecordStatus) {
        self.settled.lock().insert(id.clone(), status.clone());
        self.notify.notify_waiters();
    }
}

pub struct SwitchablePolicy {
    decision: Mutex<DrainPolicyDecision>,
}

impl SwitchablePolicy {
    pub fn new(initial: DrainPolicyDecision) -> Arc<Self> {
        Arc::new(Self {
            decision: Mutex::new(initial),
        })
    }

    pub fn set(&self, next: DrainPolicyDecision) {
        *self.decision.lock() = next;
    }
}

impl DrainAuthorizationPolicy for SwitchablePolicy {
    fn authorize_drain(&self, _frame: &OutboxFrame, _now_ms: u64) -> DrainPolicyDecision {
        self.decision.lock().clone()
    }
}
