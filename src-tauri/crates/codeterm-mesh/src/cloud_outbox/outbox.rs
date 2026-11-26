use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::account_mesh::ports::BoxFuture;

use super::model::{NextDrainAction, OutboxModel};
use super::ports::{
    CloudOutboxPort, DrainAuthorizationPolicy, DrainLifecycleObserver, DrainPolicyDecision,
    ExpiryOnlyPolicy, NoopDrainObserver, OutboxClock, ReverseStreamSink, SystemOutboxClock,
};
use super::storage::FileOutboxStore;
use super::types::{
    CompletedApplicationReply, InstallationPair, OutboundPeerRequest, OutboxEnqueueReceipt,
    OutboxError, OutboxId, OutboxLimits, OutboxRecordStatus, OutboxSnapshot, StreamDeliveryOutcome,
    UnconfirmedReason,
};

pub struct CloudOutboxConfig {
    pub limits: OutboxLimits,
    pub clock: Arc<dyn OutboxClock>,
    pub policy: Arc<dyn DrainAuthorizationPolicy>,
    pub observer: Arc<dyn DrainLifecycleObserver>,
}

impl Default for CloudOutboxConfig {
    fn default() -> Self {
        Self {
            limits: OutboxLimits::default(),
            clock: Arc::new(SystemOutboxClock),
            policy: Arc::new(ExpiryOnlyPolicy),
            observer: Arc::new(NoopDrainObserver),
        }
    }
}

struct OutboxRuntimeState {
    model: OutboxModel,
    notifies: HashMap<InstallationPair, Arc<Notify>>,
    parked_pairs: HashSet<InstallationPair>,
    closed: bool,
}

impl OutboxRuntimeState {
    fn notify_for(&mut self, pair: &InstallationPair) -> Arc<Notify> {
        self.notifies
            .entry(pair.clone())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    fn wake_pair(&self, pair: &InstallationPair) {
        if let Some(notify) = self.notifies.get(pair) {
            notify.notify_waiters();
        }
    }

    fn wake_all(&self) {
        for notify in self.notifies.values() {
            notify.notify_waiters();
        }
    }
}

struct CloudOutboxInner {
    state: Mutex<OutboxRuntimeState>,
    store: FileOutboxStore,
    clock: Arc<dyn OutboxClock>,
    policy: Arc<dyn DrainAuthorizationPolicy>,
    observer: Arc<dyn DrainLifecycleObserver>,
    settled_notify: Arc<Notify>,
}

pub struct CloudOutbox {
    inner: Arc<CloudOutboxInner>,
}

impl Drop for CloudOutbox {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        state.closed = true;
        state.wake_all();
        drop(state);
        self.inner.settled_notify.notify_waiters();
    }
}

impl CloudOutbox {
    pub async fn open(
        root: impl Into<PathBuf>,
        config: CloudOutboxConfig,
    ) -> Result<Self, OutboxError> {
        config.limits.validate()?;
        let store = FileOutboxStore::new(root);
        let loaded = store.load_all().await?;
        let now_ms = config.clock.now_ms();
        let (model, crash_transitions) = OutboxModel::from_recovered(
            config.limits,
            loaded.active,
            loaded.unconfirmed,
            loaded.settled,
            now_ms,
        )?;
        for transition in crash_transitions {
            store
                .persist_unconfirmed_and_remove_active(
                    &transition.unconfirmed,
                    transition.stale_active_sequence,
                )
                .await?;
        }
        Ok(Self {
            inner: Arc::new(CloudOutboxInner {
                state: Mutex::new(OutboxRuntimeState {
                    model,
                    notifies: HashMap::new(),
                    parked_pairs: HashSet::new(),
                    closed: false,
                }),
                store,
                clock: config.clock,
                policy: config.policy,
                observer: config.observer,
                settled_notify: Arc::new(Notify::new()),
            }),
        })
    }

    pub async fn open_default(root: impl Into<PathBuf>) -> Result<Self, OutboxError> {
        Self::open(root, CloudOutboxConfig::default()).await
    }

    pub async fn enqueue_with_id(
        &self,
        id: OutboxId,
        request: OutboundPeerRequest,
    ) -> Result<OutboxEnqueueReceipt, OutboxError> {
        let now_ms = self.inner.clock.now_ms();
        let reservation = {
            let mut state = self.inner.state.lock();
            state.model.reserve_enqueue(request, Some(id), now_ms)?
        };
        self.finish_enqueue(reservation).await.and_then(|receipts| {
            receipts
                .into_iter()
                .next()
                .ok_or_else(|| OutboxError::Storage {
                    detail: "enqueue produced no receipt".into(),
                })
        })
    }

    async fn enqueue_inner(
        &self,
        request: OutboundPeerRequest,
    ) -> Result<OutboxEnqueueReceipt, OutboxError> {
        let now_ms = self.inner.clock.now_ms();
        let reservation = {
            let mut state = self.inner.state.lock();
            state.model.reserve_enqueue(request, None, now_ms)?
        };
        self.finish_enqueue(reservation).await.and_then(|receipts| {
            receipts
                .into_iter()
                .next()
                .ok_or_else(|| OutboxError::Storage {
                    detail: "enqueue produced no receipt".into(),
                })
        })
    }

    async fn enqueue_broadcast_inner(
        &self,
        base: OutboundPeerRequest,
        target_installation_ids: Vec<String>,
    ) -> Result<Vec<OutboxEnqueueReceipt>, OutboxError> {
        let now_ms = self.inner.clock.now_ms();
        let reservation = {
            let mut state = self.inner.state.lock();
            state
                .model
                .reserve_broadcast(base, target_installation_ids, now_ms)?
        };
        self.finish_enqueue(reservation).await
    }

    async fn finish_enqueue(
        &self,
        reservation: super::model::EnqueueReservation,
    ) -> Result<Vec<OutboxEnqueueReceipt>, OutboxError> {
        let pairs: Vec<InstallationPair> = reservation
            .records
            .iter()
            .map(|record| record.frame.request.pair())
            .collect();
        if let Err(error) = self
            .inner
            .store
            .write_active_batch(&reservation.records)
            .await
        {
            let mut state = self.inner.state.lock();
            state.model.abort_enqueue(&reservation);
            return Err(error);
        }
        let receipts = {
            let mut state = self.inner.state.lock();
            let receipts = state.model.commit_enqueue(reservation);
            for pair in &pairs {
                state.parked_pairs.remove(pair);
                state.wake_pair(pair);
            }
            receipts
        };
        for receipt in &receipts {
            self.inner.observer.on_enqueued(&receipt.id);
        }
        Ok(receipts)
    }

    async fn attach_reverse_stream_inner(&self, sink: Arc<dyn ReverseStreamSink>) {
        let pair = sink.pair();
        let account_id = sink.account_id().to_string();
        let stream_id = sink.stream_id();
        let (notify, should_spawn) = {
            let mut state = self.inner.state.lock();
            if state.closed {
                return;
            }
            let notify = state.notify_for(&pair);
            if state.model.is_current_stream(&pair, stream_id) {
                notify.notify_waiters();
                (notify, false)
            } else {
                state
                    .model
                    .attach_stream(pair.clone(), account_id.clone(), stream_id);
                state.parked_pairs.remove(&pair);
                notify.notify_waiters();
                (notify, true)
            }
        };
        if !should_spawn {
            return;
        }
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            run_drain_loop(inner, pair, account_id, stream_id, sink, notify).await;
        });
    }

    async fn dismiss_unconfirmed_inner(&self, id: &OutboxId) -> Result<bool, OutboxError> {
        let removed = {
            let mut state = self.inner.state.lock();
            state.model.dismiss_unconfirmed(id)
        };
        let Some(receipt) = removed else {
            return Ok(false);
        };
        self.inner
            .store
            .remove_unconfirmed(receipt.sequence, &receipt.id)
            .await?;
        Ok(true)
    }
}

impl CloudOutboxPort for CloudOutbox {
    fn enqueue(
        &self,
        request: OutboundPeerRequest,
    ) -> BoxFuture<'_, Result<OutboxEnqueueReceipt, OutboxError>> {
        Box::pin(self.enqueue_inner(request))
    }

    fn enqueue_broadcast(
        &self,
        base: OutboundPeerRequest,
        target_installation_ids: Vec<String>,
    ) -> BoxFuture<'_, Result<Vec<OutboxEnqueueReceipt>, OutboxError>> {
        Box::pin(self.enqueue_broadcast_inner(base, target_installation_ids))
    }

    fn attach_reverse_stream(&self, sink: Arc<dyn ReverseStreamSink>) -> BoxFuture<'_, ()> {
        Box::pin(self.attach_reverse_stream_inner(sink))
    }

    fn detach_reverse_stream(&self, stream_id: u64) {
        let mut state = self.inner.state.lock();
        if let Some(pair) = state.model.detach_stream(stream_id) {
            state.parked_pairs.remove(&pair);
            state.wake_pair(&pair);
        }
        drop(state);
        self.inner.settled_notify.notify_waiters();
    }

    fn notify_policy_changed(&self) {
        let mut state = self.inner.state.lock();
        state.parked_pairs.clear();
        state.wake_all();
        drop(state);
        self.inner.settled_notify.notify_waiters();
    }

    fn record_status(&self, id: &OutboxId) -> Option<OutboxRecordStatus> {
        self.inner.state.lock().model.record_status(id)
    }

    fn wait_for_settlement<'a>(
        &'a self,
        id: &'a OutboxId,
        pair: &'a InstallationPair,
    ) -> BoxFuture<'a, Option<OutboxRecordStatus>> {
        Box::pin(async move {
            loop {
                let notified = self.inner.settled_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                {
                    let state = self.inner.state.lock();
                    let status = state.model.record_status(id);
                    match &status {
                        None
                        | Some(
                            OutboxRecordStatus::Delivered { .. }
                            | OutboxRecordStatus::DeliveryUnconfirmed { .. }
                            | OutboxRecordStatus::RefusedBeforeWrite { .. },
                        ) => return status,
                        Some(OutboxRecordStatus::Queued { .. })
                            if state.closed
                                || !state.model.has_attached_stream(pair)
                                || state.parked_pairs.contains(pair) =>
                        {
                            return status;
                        }
                        Some(
                            OutboxRecordStatus::Queued { .. } | OutboxRecordStatus::InFlight { .. },
                        ) => {
                            if state.closed {
                                return status;
                            }
                        }
                    }
                }
                notified.await;
            }
        })
    }

    fn application_reply(&self, id: &OutboxId) -> Option<CompletedApplicationReply> {
        self.inner.state.lock().model.application_reply(id)
    }

    fn unconfirmed_records(&self) -> Vec<(OutboxId, OutboxRecordStatus)> {
        self.inner.state.lock().model.unconfirmed_records()
    }

    fn dismiss_unconfirmed(&self, id: &OutboxId) -> BoxFuture<'_, Result<bool, OutboxError>> {
        let id = id.clone();
        Box::pin(async move { self.dismiss_unconfirmed_inner(&id).await })
    }

    fn snapshot(&self) -> OutboxSnapshot {
        self.inner.state.lock().model.snapshot()
    }
}

enum DrainStep {
    Continue,
    Wait,
    Exit,
}

async fn run_drain_loop(
    inner: Arc<CloudOutboxInner>,
    pair: InstallationPair,
    account_id: String,
    stream_id: u64,
    sink: Arc<dyn ReverseStreamSink>,
    notify: Arc<Notify>,
) {
    loop {
        // Register waiter before locking so concurrent enqueues or detaches cannot miss a wakeup.
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        match drain_one_step(&inner, &pair, &account_id, stream_id, &sink).await {
            DrainStep::Continue => continue,
            DrainStep::Wait => notified.await,
            DrainStep::Exit => break,
        }
    }
}

async fn drain_one_step(
    inner: &Arc<CloudOutboxInner>,
    pair: &InstallationPair,
    account_id: &str,
    stream_id: u64,
    sink: &Arc<dyn ReverseStreamSink>,
) -> DrainStep {
    let action = {
        let mut state = inner.state.lock();
        if state.closed {
            return DrainStep::Exit;
        }
        state.model.begin_next_drain(pair, account_id, stream_id)
    };

    let frame = match action {
        NextDrainAction::Evaluate(frame) => *frame,
        NextDrainAction::WaitForInFlight | NextDrainAction::Idle => return DrainStep::Wait,
        NextDrainAction::StreamSuperseded => return DrainStep::Exit,
    };

    let now_ms = inner.clock.now_ms();
    let policy_decision = if frame
        .request
        .caller_context
        .expires_at_ms
        .is_some_and(|expires| now_ms >= expires)
    {
        DrainPolicyDecision::Refuse {
            reason: "request_expired".into(),
        }
    } else {
        inner.policy.authorize_drain(&frame, now_ms)
    };

    match policy_decision {
        DrainPolicyDecision::Defer { .. } => {
            let mut state = inner.state.lock();
            state.model.revert_evaluating(&frame.id, stream_id);
            state.parked_pairs.insert(pair.clone());
            drop(state);
            inner.settled_notify.notify_waiters();
            return DrainStep::Wait;
        }
        DrainPolicyDecision::Refuse { reason } => {
            let (settled, retain) = {
                let mut state = inner.state.lock();
                state.parked_pairs.remove(pair);
                let retain = state.model.limits().max_retained_settled > 0;
                let res = state
                    .model
                    .settle_refused_before_write(&frame.id, reason, now_ms);
                (res, retain)
            };
            if let Some((receipt, evicted)) = settled {
                if let Err(error) = inner
                    .store
                    .persist_settled_and_remove_active(
                        retain.then_some(&receipt),
                        frame.sequence,
                        &frame.id,
                        &evicted,
                    )
                    .await
                {
                    log::error!(
                        "[cloud-outbox] refused settlement persist failed id={}: {error}",
                        frame.id
                    );
                }
                inner.state.lock().wake_pair(pair);
                inner.observer.on_settled(&frame.id, &receipt.status);
                inner.settled_notify.notify_waiters();
            }
            return DrainStep::Continue;
        }
        DrainPolicyDecision::Allow => {}
    }

    let claimed_record = {
        let mut state = inner.state.lock();
        if state.closed {
            state.model.revert_evaluating(&frame.id, stream_id);
            return DrainStep::Exit;
        }
        state.parked_pairs.remove(pair);
        state
            .model
            .prepare_claim(pair, &frame.id, stream_id, now_ms)
    };
    let Some(claimed_record) = claimed_record else {
        return DrainStep::Exit;
    };

    if let Err(error) = inner.store.write_active(&claimed_record).await {
        log::error!(
            "[cloud-outbox] durable claim persist failed id={}: {error}",
            frame.id
        );
        let mut state = inner.state.lock();
        state.model.revert_unpersisted_claim(&frame.id, stream_id);
        state.parked_pairs.insert(pair.clone());
        state.wake_pair(pair);
        drop(state);
        inner.settled_notify.notify_waiters();
        return DrainStep::Wait;
    }

    inner.observer.on_claimed_persisted(&frame.id, stream_id);

    let outcome = sink.deliver(&frame).await;
    inner
        .observer
        .on_delivered_before_settle(&frame.id, &outcome);

    let settle_ms = inner.clock.now_ms();
    match outcome {
        StreamDeliveryOutcome::Acked { reply_body } => {
            let (settled, retain) = {
                let mut state = inner.state.lock();
                let retain = state.model.limits().max_retained_settled > 0;
                let res = state.model.settle_acked(&frame.id, reply_body, settle_ms);
                (res, retain)
            };
            if let Some((receipt, evicted)) = settled {
                if let Err(error) = inner
                    .store
                    .persist_settled_and_remove_active(
                        retain.then_some(&receipt),
                        frame.sequence,
                        &frame.id,
                        &evicted,
                    )
                    .await
                {
                    log::error!(
                        "[cloud-outbox] ack settlement persist failed id={}: {error}",
                        frame.id
                    );
                }
                inner.state.lock().wake_pair(pair);
                inner.observer.on_settled(&frame.id, &receipt.status);
                inner.settled_notify.notify_waiters();
            }
            DrainStep::Continue
        }
        StreamDeliveryOutcome::NotWritten { .. } => {
            let queued_copy = inner.state.lock().model.persisted_queued_copy(&frame.id);
            let persisted_rollback = match queued_copy {
                Some(record) => inner.store.write_active(&record).await,
                None => Ok(()),
            };
            match persisted_rollback {
                Ok(()) => {
                    let mut state = inner.state.lock();
                    state.model.settle_not_written(&frame.id, stream_id);
                    state.parked_pairs.insert(pair.clone());
                    state.wake_pair(pair);
                    drop(state);
                    inner.settled_notify.notify_waiters();
                }
                Err(error) => {
                    log::error!(
                        "[cloud-outbox] not_written rollback persist failed id={}: {error}",
                        frame.id
                    );
                    let unconfirmed = {
                        let mut state = inner.state.lock();
                        state.model.settle_unconfirmed(
                            &frame.id,
                            UnconfirmedReason::TransportUncertain,
                            None,
                            Some("failed to persist not_written rollback".into()),
                            settle_ms,
                        )
                    };
                    if let Some(receipt) = unconfirmed {
                        let _ = inner
                            .store
                            .persist_unconfirmed_and_remove_active(&receipt, frame.sequence)
                            .await;
                        inner.state.lock().wake_pair(pair);
                        inner.observer.on_settled(&frame.id, &receipt.into_status());
                        inner.settled_notify.notify_waiters();
                    }
                }
            }
            DrainStep::Wait
        }
        StreamDeliveryOutcome::Unconfirmed {
            reason,
            bytes_written,
            detail,
        } => {
            let unconfirmed = {
                let mut state = inner.state.lock();
                state
                    .model
                    .settle_unconfirmed(&frame.id, reason, bytes_written, detail, settle_ms)
            };
            if let Some(receipt) = unconfirmed {
                if let Err(error) = inner
                    .store
                    .persist_unconfirmed_and_remove_active(&receipt, frame.sequence)
                    .await
                {
                    log::error!(
                        "[cloud-outbox] unconfirmed settlement persist failed id={}: {error}",
                        frame.id
                    );
                }
                inner.state.lock().wake_pair(pair);
                inner.observer.on_settled(&frame.id, &receipt.into_status());
                inner.settled_notify.notify_waiters();
            }
            if reason.stream_likely_broken() {
                DrainStep::Wait
            } else {
                DrainStep::Continue
            }
        }
    }
}
