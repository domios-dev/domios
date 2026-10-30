use super::key::InstallationKey;
use super::membership::{account_id, MembershipTrust};
use super::operations::request_id;
use super::ports::{
    AccountMeshStore, Clock, EndpointSource, InstallationKeyStore, MeshRegistry, MeshTransport,
    SessionSource,
};
use super::state::{
    AccountMeshRecord, AccountMeshRefusal, Cleanup, EnrollTrigger, Enrollment, LifecycleState,
    LimitNotice, Presence, Publication,
};
use super::wire::{MeshTarget, RegistryPresence};
use crate::{Identity, InstallationId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{watch, Notify};

pub struct AccountMeshConfig {
    pub installation_id: InstallationId,
    pub default_display_name: Option<String>,
    /// `None` until a release pins issuer trust: nothing is enrolled or admitted.
    pub trust: Option<MembershipTrust>,
    pub call_deadline: Duration,
    pub withdrawal_deadline: Duration,
    pub schedule_leases: bool,
}

pub struct AccountMeshPorts {
    pub registry: Arc<dyn MeshRegistry>,
    pub sessions: Arc<dyn SessionSource>,
    pub keys: Arc<dyn InstallationKeyStore>,
    pub store: Arc<dyn AccountMeshStore>,
    pub endpoint: Arc<dyn EndpointSource>,
    pub transport: Arc<dyn MeshTransport>,
    pub clock: Arc<dyn Clock>,
}

#[derive(Clone, Default)]
pub(super) struct Cancel(Arc<(AtomicBool, Notify)>);
impl Cancel {
    pub(super) fn cancel(&self) {
        self.0 .0.store(true, Ordering::SeqCst);
        self.0 .1.notify_waiters();
    }
    pub(super) fn is_cancelled(&self) -> bool {
        self.0 .0.load(Ordering::SeqCst)
    }
    pub(super) async fn cancelled(&self) {
        loop {
            let notified = self.0 .1.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
    fn same(&self, other: &Cancel) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

type Settled = Result<(), AccountMeshRefusal>;

struct Flight {
    epoch: u64,
    cancel: Cancel,
    rerun: Option<EnrollTrigger>,
    outcome: watch::Receiver<Option<Settled>>,
}

enum Start {
    Owner(u64, Cancel, watch::Sender<Option<Settled>>),
    Joined(watch::Receiver<Option<Settled>>),
}

/// Clears the sign-out guard however the release ends, so a failed or abandoned logout
/// cannot wedge later sign-ins.
struct LogoutWindow<'a>(&'a AccountMesh);
impl Drop for LogoutWindow<'_> {
    fn drop(&mut self) {
        self.0.with(|inner| inner.awaiting_logout = false);
    }
}

/// An owner dropped mid-run (its request cancelled) frees the flight instead of leaving later
/// triggers joined to a run that never settles.
struct FlightOwner<'a> {
    mesh: &'a AccountMesh,
    cancel: Cancel,
}
impl Drop for FlightOwner<'_> {
    fn drop(&mut self) {
        self.mesh.with(|inner| {
            if inner
                .flight
                .as_ref()
                .is_some_and(|flight| flight.cancel.same(&self.cancel))
            {
                inner.flight = None;
            }
        });
    }
}

pub(super) struct Inner {
    pub(super) state: Arc<LifecycleState>,
    pub(super) record: AccountMeshRecord,
    pub(super) key: Option<Arc<InstallationKey>>,
    pub(super) rotation_attempt: Option<u64>,
    pub(super) rejoin_required: bool,
    pub(super) rejoin_authorized: bool,
    pub(super) last_request: Option<String>,
    pub(super) pending_renew: Option<super::wire::MeshAdmissionBody>,
    pub(super) metadata_revision: Option<u64>,
    pub(super) registry_presence: Option<RegistryPresence>,
    pub(super) registry_notices: super::notice::RegistryNotices,
    pub(super) registry_hints: super::registry_hint::RegistryHints,
    pub(super) registry_flight: Option<super::registry_read::RegistryFlight>,
    pub(super) published_endpoint: Option<super::wire::MeshEndpoint>,
    pub(super) published_until: u64,
    pub(super) publication_refresh_after: u64,
    pub(super) sync_pending: bool,
    pub(super) refreshed_credentials: Option<super::admission::CredentialStamp>,
    pub(super) auth_parked: Option<super::admission::CredentialStamp>,
    pub(super) resolved: HashMap<String, MeshTarget>,
    flight: Option<Flight>,
    awaiting_logout: bool,
    notice_epoch: Option<u64>,
    pub(super) lease: u64,
    pub(super) retry: super::retry::Retry,
    store_unreadable: bool,
}

impl Inner {
    pub(super) fn update(
        &mut self,
        watch: &watch::Sender<Arc<LifecycleState>>,
        change: impl FnOnce(&mut LifecycleState),
    ) {
        let mut next = (*self.state).clone();
        change(&mut next);
        next.unavailable_installations = self
            .record
            .peer_presence
            .iter()
            .filter(|(installation, presence)| {
                presence.presence == RegistryPresence::SignedOut
                    && !self.record.revoked_installations.contains(*installation)
            })
            .map(|(installation, _)| installation.clone())
            .collect();
        if next.registry != self.state.registry {
            match next.registry.as_ref() {
                Some(registry) => {
                    if let Some(security) = registry.security_view() {
                        if next
                            .registry_security
                            .as_ref()
                            .is_none_or(|known| security.revision >= known.revision)
                        {
                            next.registry_security = Some(security);
                        }
                    }
                }
                None => next.registry_security = None,
            }
        }
        self.state = Arc::new(next);
        watch.send_replace(self.state.clone());
    }

    fn cancel_flight(&mut self) {
        if let Some(flight) = self.flight.take() {
            flight.cancel.cancel();
        }
    }

    /// A response may commit only while its account epoch is still the signed-in one.
    pub(super) fn is_current(&self, epoch: u64) -> bool {
        self.state.account_epoch == epoch && self.state.signed_in_account().is_some()
    }

    fn begin_epoch(&mut self) {
        self.cancel_flight();
        self.auth_parked = None;
        self.rejoin_authorized = false;
        self.pending_renew = None;
        self.refreshed_credentials = None;
        self.notice_epoch = None;
        self.registry_flight = None;
        self.retry = Default::default();
        self.metadata_revision = None;
        self.registry_presence = None;
        self.resolved.clear();
        self.published_endpoint = None;
        self.published_until = 0;
        self.publication_refresh_after = 0;
        self.sync_pending = false;
    }
}

/// Owns enrollment, publication and the sign-out block for one installation.
pub struct AccountMesh {
    pub(super) config: AccountMeshConfig,
    pub(super) ports: AccountMeshPorts,
    pub(super) inner: Mutex<Inner>,
    pub(super) watch: watch::Sender<Arc<LifecycleState>>,
    persist_lock: tokio::sync::Mutex<()>,
    push_lock: tokio::sync::Mutex<()>,
    pub(super) key_lock: tokio::sync::Mutex<()>,
    pub(super) admission_lock: tokio::sync::Mutex<()>,
}

fn merge(current: Option<EnrollTrigger>, next: EnrollTrigger) -> EnrollTrigger {
    let rank = |trigger: EnrollTrigger| match trigger {
        EnrollTrigger::Explicit => 3,
        EnrollTrigger::Automatic => 2,
        EnrollTrigger::Lease => 1,
        EnrollTrigger::Network => 0,
    };
    match current {
        Some(current) if rank(current) >= rank(next) => current,
        _ => next,
    }
}

impl AccountMesh {
    pub fn new(config: AccountMeshConfig, ports: AccountMeshPorts) -> Arc<Self> {
        let state = Arc::new(LifecycleState::default());
        Arc::new(Self {
            config,
            ports,
            inner: Mutex::new(Inner {
                state: state.clone(),
                record: AccountMeshRecord::default(),
                key: None,
                rotation_attempt: None,
                rejoin_required: false,
                rejoin_authorized: false,
                last_request: None,
                pending_renew: None,
                metadata_revision: None,
                registry_presence: None,
                registry_notices: Default::default(),
                registry_hints: Default::default(),
                registry_flight: None,
                resolved: HashMap::new(),
                published_endpoint: None,
                published_until: 0,
                publication_refresh_after: 0,
                sync_pending: false,
                auth_parked: None,
                refreshed_credentials: None,
                flight: None,
                awaiting_logout: false,
                notice_epoch: None,
                lease: 0,
                retry: Default::default(),
                store_unreadable: false,
            }),
            watch: watch::channel(state).0,
            persist_lock: tokio::sync::Mutex::new(()),
            push_lock: tokio::sync::Mutex::new(()),
            key_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub(super) fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        f(&mut inner)
    }

    pub fn state(&self) -> Arc<LifecycleState> {
        self.with(|inner| inner.state.clone())
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<LifecycleState>> {
        self.watch.subscribe()
    }

    pub(super) fn now(&self) -> u64 {
        self.ports.clock.now()
    }

    pub(super) async fn persist(&self) {
        let _guard = self.persist_lock.lock().await;
        let record = self.with(|inner| {
            inner.cache_admission();
            (!inner.store_unreadable).then(|| inner.record.clone())
        });
        let Some(record) = record else {
            return;
        };
        if let Err(error) = self.ports.store.save(&record).await {
            log::debug!(
                "[account-mesh] lifecycle persistence failed: {:?}",
                error.kind()
            );
        }
    }

    async fn persist_checked(&self) -> Result<(), String> {
        let _guard = self.persist_lock.lock().await;
        let record = self
            .with(|inner| (!inner.store_unreadable).then(|| inner.record.clone()))
            .ok_or_else(|| "Account Mesh sign-out could not be persisted".to_owned())?;
        self.ports
            .store
            .save(&record)
            .await
            .map_err(|_| "Account Mesh sign-out could not be persisted".to_owned())
    }

    /// Transport always receives the latest snapshot, in order, after each change.
    pub(super) async fn push(&self) {
        let _guard = self.push_lock.lock().await;
        let state = self.watch.borrow().clone();
        self.ports.transport.apply(&state).await;
    }

    /// Unreadable durable state fails closed: Mesh stays blocked until a verified sign-in.
    pub async fn load(&self) {
        let loaded = self.ports.store.load().await;
        self.with(|inner| {
            let unreadable = loaded.is_err();
            inner.store_unreadable = unreadable;
            let mut record = loaded.unwrap_or_else(|_| AccountMeshRecord {
                signed_out: true,
                ..AccountMeshRecord::default()
            });
            let own = self.installation();
            record.peer_presence.remove(&own);
            if !record.revoked_generations.contains_key(&own) {
                record.revoked_installations.remove(&own);
            }
            for installation in record.peer_presence.keys() {
                if !record.revoked_generations.contains_key(installation) {
                    record.revoked_installations.remove(installation);
                }
            }
            inner.record = record.clone();
            inner.update(&self.watch, |state| {
                state.presence = if record.signed_out {
                    Presence::SignedOut
                } else {
                    Presence::NeverSignedIn
                };
                state.owner = record.owner.clone();
                state.revoked_installations = record.revoked_installations.clone();
                state.cleanup = if record.cleanup_pending {
                    Cleanup::Pending
                } else {
                    Cleanup::None
                };
                state.enrollment = if unreadable {
                    Enrollment::Refused
                } else {
                    Enrollment::Idle
                };
                state.last_refusal = unreadable.then_some(AccountMeshRefusal::Unavailable);
            });
        });
        self.push().await;
    }

    /// Identity observer path: cheap, never performs registry IO, never fails sign-in.
    pub async fn identity_changed(&self, identity: Option<&Identity>) {
        let changed = self.with(|inner| match identity {
            Some(identity) => {
                let account = account_id(&identity.uid);
                if inner.awaiting_logout
                    || inner.state.signed_in_account() == Some(account.as_str())
                {
                    return false;
                }
                inner.begin_epoch();
                inner.rejoin_required = false;
                if inner.record.signed_out {
                    inner.record.admission_cache = None;
                }
                inner.record.signed_out = false;
                let owner = inner
                    .record
                    .owner
                    .get_or_insert_with(|| account.clone())
                    .clone();
                let store_unreadable = inner.store_unreadable;
                inner.update(&self.watch, |state| {
                    state.account_epoch += 1;
                    state.presence = Presence::SignedIn {
                        account_id: account,
                    };
                    state.owner = Some(owner);
                    state.membership = None;
                    state.enrollment = if store_unreadable {
                        Enrollment::Refused
                    } else {
                        Enrollment::Idle
                    };
                    state.last_refusal =
                        store_unreadable.then_some(AccountMeshRefusal::Unavailable);
                    state.limit_notice = None;
                    state.self_installation = None;
                    state.registry = None;
                    state.registry_failure = None;
                    state.publication = Publication::None;
                    state.publication_failure = None;
                    state.retry_at = None;
                });
                true
            }
            None => {
                inner.awaiting_logout = false;
                if inner.state.signed_in_account().is_none() {
                    return false;
                }
                // Credentials vanished without the explicit path: block, but the token is gone.
                Self::block(inner, &self.watch);
                true
            }
        });
        if changed {
            self.persist().await;
            self.push().await;
        }
    }

    fn block(inner: &mut Inner, watch: &watch::Sender<Arc<LifecycleState>>) {
        inner.begin_epoch();
        inner.record.admission_cache = None;
        inner.record.signed_out = true;
        inner.record.cleanup_pending = inner.record.published;
        let cleanup = if inner.record.published {
            Cleanup::Pending
        } else {
            Cleanup::Confirmed
        };
        inner.update(watch, |state| {
            state.account_epoch += 1;
            state.presence = Presence::SignedOut;
            state.membership = None;
            state.enrollment = Enrollment::Idle;
            state.retry_at = None;
            state.publication_failure = None;
            state.limit_notice = None;
            state.registry = None;
            state.cleanup = cleanup;
        });
    }

    /// Explicit sign-out: block durably, withdraw once while the account's credentials still
    /// exist, then `release` them. Only identity changes racing the release are ignored.
    pub async fn sign_out<T>(
        &self,
        release: impl Future<Output = T>,
    ) -> (Cleanup, Result<(), String>, T) {
        let _window = LogoutWindow(self);
        let (cleanup, persisted) = self.block_and_withdraw().await;
        (cleanup, persisted, release.await)
    }

    pub async fn sign_out_local(&self) -> Result<(), String> {
        self.with(|inner| Self::block(inner, &self.watch));
        self.push().await;
        self.persist_checked().await
    }

    /// Without a signed-in account there is nothing to sign out of, so legacy continuity stays.
    async fn block_and_withdraw(&self) -> (Cleanup, Result<(), String>) {
        let session = self.ports.sessions.session().await.ok().flatten();
        let Some((account, published)) = self.with(|inner| {
            let account = inner.state.signed_in_account()?.to_owned();
            inner.awaiting_logout = true;
            Self::block(inner, &self.watch);
            Some((account, inner.record.published))
        }) else {
            return (self.state().cleanup, Ok(()));
        };
        self.push().await;
        let mut persisted = self.persist_checked().await;
        let session = session.filter(|session| account == account_id(&session.identity.uid));
        if let Some(session) = session.as_ref() {
            let _ = tokio::time::timeout(
                self.config.withdrawal_deadline,
                self.announce_signed_out(session),
            )
            .await;
        }
        let cleanup = if !published {
            Cleanup::Confirmed
        } else {
            match session {
                Some(session) => {
                    let withdrawal = self.withdraw_endpoint(&session);
                    match tokio::time::timeout(self.config.withdrawal_deadline, withdrawal).await {
                        Ok(Ok(())) => Cleanup::Confirmed,
                        _ => Cleanup::Pending,
                    }
                }
                None => Cleanup::Pending,
            }
        };
        self.with(|inner| {
            inner.record.cleanup_pending = cleanup == Cleanup::Pending;
            if cleanup == Cleanup::Confirmed {
                inner.record.published = false;
            }
            inner.update(&self.watch, |state| {
                state.cleanup = cleanup;
                state.publication = Publication::None;
            });
        });
        if let Err(error) = self.persist_checked().await {
            persisted = Err(error);
        }
        self.push().await;
        (cleanup, persisted)
    }

    pub fn enrollment_signed_in(&self) {
        self.with(|inner| {
            if inner.state.signed_in_account().is_none() { return; }
            inner.rejoin_required |= matches!(inner.state.last_refusal,
                Some(AccountMeshRefusal::Revoked | AccountMeshRefusal::GenerationMismatch | AccountMeshRefusal::KeyConflict));
            inner.begin_epoch();
            inner.record.admission_cache = None;
            inner.rejoin_authorized = true;
            inner.rotation_attempt = None;
            inner.lease += 1;
            inner.update(&self.watch, |state| {
                state.account_epoch += 1;
                state.membership = None;
                state.last_refusal = None;
                state.publication_failure = None;
                state.registry_failure = None;
                state.enrollment = Enrollment::Idle;
                state.retry_at = None;
            });
        });
    }

    pub async fn refresh(self: &Arc<Self>, trigger: EnrollTrigger) -> Settled {
        let started = self.with(|inner| {
            let Presence::SignedIn { .. } = inner.state.presence else {
                return Err(match inner.state.presence {
                    Presence::SignedOut => AccountMeshRefusal::SignedOut,
                    _ => AccountMeshRefusal::Unauthenticated,
                });
            };
            if inner.state.last_refusal == Some(AccountMeshRefusal::ReauthRequired) {
                return Err(AccountMeshRefusal::ReauthRequired);
            }
            if trigger == EnrollTrigger::Explicit {
                inner.auth_parked = None;
                if inner.retry.deadline.is_none()
                    && matches!(
                        inner.retry.refusal,
                        Some(AccountMeshRefusal::EnrollmentBudget { .. })
                    )
                {
                    inner.retry = Default::default();
                }
            }
            if inner.rejoin_required && !inner.rejoin_authorized && inner.state.membership.is_none() {
                return Err(inner.state.last_refusal.unwrap_or(AccountMeshRefusal::ReauthRequired));
            }
            if inner.auth_parked.is_some() {
                return Err(AccountMeshRefusal::Unauthenticated);
            }
            let epoch = inner.state.account_epoch;
            if matches!(trigger, EnrollTrigger::Explicit | EnrollTrigger::Network) {
                inner.retry.resume();
            }
            if let Some(refusal) = inner.retry.pending(self.now()) {
                return Err(refusal);
            }
            if let Some(flight) = inner.flight.as_mut() {
                if flight.epoch == epoch && !flight.cancel.is_cancelled() {
                    flight.rerun = Some(merge(flight.rerun, trigger));
                    return Ok(Start::Joined(flight.outcome.clone()));
                }
            }
            inner.lease += 1;
            let cancel = Cancel::default();
            let (outcome, settled) = watch::channel(None);
            inner.flight = Some(Flight {
                epoch,
                cancel: cancel.clone(),
                rerun: None,
                outcome: settled,
            });
            inner.update(&self.watch, |state| {
                state.enrollment = Enrollment::Enrolling
            });
            Ok(Start::Owner(epoch, cancel, outcome))
        })?;
        let (epoch, cancel, outcome) = match started {
            Start::Owner(epoch, cancel, outcome) => (epoch, cancel, outcome),
            Start::Joined(_) if trigger != EnrollTrigger::Explicit => return Ok(()),
            Start::Joined(settled) => return Self::joined(settled).await,
        };
        let _owner = FlightOwner {
            mesh: self,
            cancel: cancel.clone(),
        };
        self.push().await;
        let mut trigger = trigger;
        loop {
            let result = tokio::select! {
                _ = cancel.cancelled() => Err(AccountMeshRefusal::Cancelled),
                result = self.enroll_once(epoch, trigger) => result,
            };
            let next = self.settle(epoch, trigger, &result);
            self.push().await;
            match next {
                Some(rerun) => trigger = rerun,
                None => {
                    self.schedule_lease(epoch, &result);
                    outcome.send_replace(Some(result));
                    return result;
                }
            }
        }
    }

    /// The outcome of the run a trigger joined, including any rerun it asked for.
    async fn joined(mut settled: watch::Receiver<Option<Settled>>) -> Settled {
        let outcome = match settled.wait_for(Option::is_some).await {
            Ok(outcome) => *outcome,
            Err(_) => None,
        };
        outcome.unwrap_or(Err(AccountMeshRefusal::Cancelled))
    }

    fn settle(
        &self,
        epoch: u64,
        trigger: EnrollTrigger,
        result: &Result<(), AccountMeshRefusal>,
    ) -> Option<EnrollTrigger> {
        self.with(|inner| {
            let owned = inner
                .flight
                .as_ref()
                .is_some_and(|flight| flight.epoch == epoch);
            if !inner.is_current(epoch) {
                if owned {
                    inner.flight = None;
                }
                return None;
            }
            if matches!(result, Err(AccountMeshRefusal::GenerationMismatch | AccountMeshRefusal::KeyConflict | AccountMeshRefusal::Revoked)) {
                inner.rejoin_required = true;
                inner.rejoin_authorized = false;
            }
            inner.retry.settle(self.now(), *result);
            if inner.state.membership.is_some() {
                inner.retry.keep_recovering(self.now());
            }
            let retry_at = inner.retry.deadline;
            let rerun = if owned
                && inner.auth_parked.is_none()
                && retry_at.is_none()
                && !inner.retry.exhausted()
                && inner.retry.pending(self.now()).is_none()
                && *result != Err(AccountMeshRefusal::ReauthRequired)
            {
                inner
                    .flight
                    .as_mut()
                    .and_then(|flight| flight.rerun.take())
                    .map(|trigger| {
                        if result.is_ok() {
                            EnrollTrigger::Network
                        } else {
                            trigger
                        }
                    })
            } else {
                None
            };
            let notice = match result {
                Err(AccountMeshRefusal::MachineLimit { .. }) => match trigger {
                    EnrollTrigger::Explicit => true,
                    EnrollTrigger::Automatic => inner.notice_epoch != Some(epoch),
                    EnrollTrigger::Lease | EnrollTrigger::Network => false,
                },
                _ => false,
            };
            let notice = match (notice, result) {
                (true, Err(refusal)) => {
                    if trigger == EnrollTrigger::Automatic {
                        inner.notice_epoch = Some(epoch);
                    }
                    Some(LimitNotice {
                        request_id: inner.last_request.clone().unwrap_or_else(request_id),
                        refusal: *refusal,
                        trigger,
                    })
                }
                _ => None,
            };
            inner.update(&self.watch, |state| {
                state.retry_at = retry_at;
                match result {
                    Ok(()) => {
                        state.enrollment = Enrollment::Enrolled;
                        state.last_refusal = None;
                    }
                    Err(AccountMeshRefusal::Cancelled) => {}
                    Err(refusal) => {
                        state.enrollment = Enrollment::Refused;
                        state.last_refusal = Some(*refusal);
                        if notice.is_some() {
                            state.limit_notice = notice;
                        }
                    }
                }
            });
            if rerun.is_none() && owned {
                inner.flight = None;
            }
            rerun
        })
    }

    /// The plans modal consumes a notice once; later reads of the same state never reopen it.
    pub async fn acknowledge_limit_notice(&self, request_id: &str) -> bool {
        let cleared = self.with(|inner| {
            if inner
                .state
                .limit_notice
                .as_ref()
                .map(|n| n.request_id.as_str())
                != Some(request_id)
            {
                return false;
            }
            inner.update(&self.watch, |state| state.limit_notice = None);
            true
        });
        if cleared {
            self.push().await;
        }
        cleared
    }
}
