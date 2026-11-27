use axum::body::{to_bytes, Body};
use axum::http::{header, Request};
use codeterm_mesh::account_mesh::BoxFuture;
use codeterm_mesh::cloud_ingress::{
    connect_enrolled, connect_enrolled_with_timeout, CloudRoute, IngressError, ReverseDispatcher,
};
use codeterm_mesh::cloud_outbox::{
    DedupeLimits, DrainAuthorizationPolicy, DrainPolicyDecision, OutboundOperation, OutboxFrame,
    OuterWireRequest, ReceiverDedupe, OUTER_RELAY_ROUTE,
};
use codeterm_mesh::crown::AuthenticatedOrigin;
use codeterm_mesh::{MeshManager, MeshState};
use codeterm_service_core::{
    ports::rest::LocalApiRouter,
    service::{cloud_machines::CloudLifecycleState, ServiceContext, ServiceState},
};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};
use tower::ServiceExt;

struct Dispatcher(Weak<ServiceContext>);
pub fn dispatcher(service: &ServiceState) -> Arc<dyn ReverseDispatcher> {
    Arc::new(Dispatcher(Arc::downgrade(service)))
}

struct LiveDrainPolicy {
    service: Weak<ServiceContext>,
    mesh: MeshState,
}

/// Live caller authorization policy installed once at composition via
/// `CloudIngressHandle::bind_drain_policy`.
pub fn drain_policy(service: &ServiceState, mesh: MeshState) -> Arc<dyn DrainAuthorizationPolicy> {
    Arc::new(LiveDrainPolicy {
        service: Arc::downgrade(service),
        mesh,
    })
}

impl DrainAuthorizationPolicy for LiveDrainPolicy {
    fn authorize_drain(&self, frame: &OutboxFrame, now_ms: u64) -> DrainPolicyDecision {
        let Some(service) = self.service.upgrade() else {
            return DrainPolicyDecision::Defer {
                reason: "service_unavailable".into(),
            };
        };
        if !service.configuration().mesh.enabled {
            return DrainPolicyDecision::Defer {
                reason: "mesh_disabled".into(),
            };
        }
        if frame.request.operation == OutboundOperation::Notification {
            return DrainPolicyDecision::Refuse {
                reason: crate::mesh_routes::relay::CLOUD_NOTIFICATION_RELAY_REFUSED.into(),
            };
        }
        if frame
            .request
            .caller_context
            .expires_at_ms
            .is_some_and(|expires_at_ms| now_ms >= expires_at_ms)
        {
            return DrainPolicyDecision::Refuse {
                reason: "request_expired".into(),
            };
        }
        if frame.request.validate().is_err() {
            return DrainPolicyDecision::Refuse {
                reason: "invalid_outbox_request".into(),
            };
        }
        if frame.request.caller_context.origin == AuthenticatedOrigin::Agent {
            let Some(agent) = frame.request.caller_context.agent.as_ref() else {
                return DrainPolicyDecision::Refuse {
                    reason: "caller_agent_missing".into(),
                };
            };
            if service.mux.read().get_tab(&agent.tab_id).is_none() {
                return DrainPolicyDecision::Refuse {
                    reason: "caller_tab_closed".into(),
                };
            }
        }
        let mesh = self.mesh.lock();
        let account = mesh.account_state();
        if account.signed_in_account() != Some(frame.request.account_id.as_str()) {
            return DrainPolicyDecision::Refuse {
                reason: "account_changed".into(),
            };
        }
        if account
            .revoked_installations
            .contains(&frame.request.target_installation_id)
        {
            return DrainPolicyDecision::Refuse {
                reason: "target_revoked".into(),
            };
        }
        if frame.request.caller_context.origin == AuthenticatedOrigin::Agent
            && !frame
                .request
                .caller_context
                .agent
                .as_ref()
                .is_some_and(|agent| agent.instance_id == mesh.ident.identity.instance_id)
        {
            return DrainPolicyDecision::Refuse {
                reason: "caller_instance_mismatch".into(),
            };
        }
        let target_id = frame.request.target_installation_id.as_str();
        let paired_peer_id = mesh.paired_peers.keys().find(|peer_id| {
            mesh.link_record(peer_id)
                .and_then(|record| record.installation_id())
                == Some(target_id)
        });
        let Some(peer_id) = paired_peer_id else {
            let excluded_by_registry = account
                .registry_members
                .as_ref()
                .is_some_and(|members| !members.contains_key(target_id));
            let unpaired_in_inventory = !mesh.account_installations.is_empty()
                && !mesh
                    .account_installations
                    .iter()
                    .any(|row| row.active && !row.signed_out && row.installation_id == target_id);
            return if excluded_by_registry || unpaired_in_inventory {
                DrainPolicyDecision::Refuse {
                    reason: "target_unpaired_or_revoked".into(),
                }
            } else {
                DrainPolicyDecision::Defer {
                    reason: "target_peer_not_paired".into(),
                }
            };
        };
        match mesh.traffic_decision(peer_id) {
            codeterm_mesh::account_mesh::gate::TrafficDecision::Open => DrainPolicyDecision::Allow,
            codeterm_mesh::account_mesh::gate::TrafficDecision::Blocked(
                codeterm_mesh::account_mesh::gate::BlockReason::Revoked
                | codeterm_mesh::account_mesh::gate::BlockReason::KeyMismatch
                | codeterm_mesh::account_mesh::gate::BlockReason::OtherAccount
                | codeterm_mesh::account_mesh::gate::BlockReason::NoProvenance,
            ) => DrainPolicyDecision::Refuse {
                reason: "target_revoked".into(),
            },
            codeterm_mesh::account_mesh::gate::TrafficDecision::Blocked(_) => {
                DrainPolicyDecision::Defer {
                    reason: "peer_traffic_closed".into(),
                }
            }
        }
    }
}

fn is_relayed_notification(body: &serde_json::Value) -> bool {
    let method = body
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let path = body
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST") && bare == "/api/notifications"
}

impl ReverseDispatcher for Dispatcher {
    fn enabled(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|s| s.configuration().mesh.enabled)
    }
    fn dispatch(
        &self,
        request: OuterWireRequest,
        signed_token: String,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, IngressError>> {
        Box::pin(async move {
            let service = self.0.upgrade().ok_or(IngressError::Unavailable)?;
            if !service.configuration().mesh.enabled {
                return Err(IngressError::Rejected);
            }
            // Refused pending security review on cloud-relayed notification authority.
            if request.route == OUTER_RELAY_ROUTE && is_relayed_notification(&request.body) {
                return Err(IngressError::Rejected);
            }
            let router = service
                .try_ext::<LocalApiRouter>()
                .ok_or(IngressError::Unavailable)?
                .0
                .clone();
            let body = serde_json::to_vec(&request.body).map_err(|_| IngressError::InvalidFrame)?;
            let request = Request::builder()
                .method(request.route.method)
                .uri(request.route.path)
                .header(header::AUTHORIZATION, format!("Bearer {signed_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .map_err(|_| IngressError::InvalidFrame)?;
            let response = router
                .oneshot(request)
                .await
                .unwrap_or_else(|never| match never {});
            if !response.status().is_success() {
                return Err(IngressError::Rejected);
            }
            let body = to_bytes(
                response.into_body(),
                DedupeLimits::default().max_reply_bytes,
            )
            .await
            .map_err(|_| IngressError::InvalidFrame)?;
            Ok((!body.is_empty()).then(|| body.to_vec()))
        })
    }
}

async fn maintain(
    mesh: MeshState,
    id: String,
    route: CloudRoute,
    dedupe: Arc<ReceiverDedupe>,
    dispatcher: Arc<Dispatcher>,
    connect_timeout: Duration,
    attached: &mut bool,
) -> Result<(), IngressError> {
    let mut channel =
        connect_enrolled_with_timeout(mesh.clone(), route.clone(), &id, connect_timeout).await?;
    let mut previous_expiry = None;
    loop {
        let member = channel.peer_membership()?;
        let expiry = member.expires_at;
        if previous_expiry.is_some_and(|prior| expiry <= prior) {
            return Err(IngressError::Rejected);
        }
        let refresh = expiry.saturating_sub(expiry.saturating_sub(member.issued_at) / 12);
        let delay = Duration::from_secs(refresh.saturating_sub(mesh.lock().now()));
        tokio::select! {
            result=channel.receive_reverse_with_attach(&dedupe,dispatcher.as_ref(), || *attached = true)=>return result,
            _=tokio::time::sleep(delay)=>{}
        }
        connect_enrolled(mesh.clone(), route.clone(), &id)
            .await?
            .renew()
            .await?;
        connect_enrolled(mesh.clone(), route.clone(), &id)
            .await?
            .publish_endpoint(&route)
            .await?;
        previous_expiry = Some(expiry);
        channel = connect_enrolled(mesh.clone(), route.clone(), &id).await?;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteLifecycle {
    Unmanaged,
    Waking {
        deadline_ms: u64,
        ingress_ready_at: u64,
    },
    LegacyWake {
        deadline_ms: u64,
    },
    Awake {
        since_ms: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RouteAttempt {
    sequence: u64,
    lifecycle: RouteLifecycle,
}

#[derive(Clone, Copy)]
struct RouteDial {
    attempt: RouteAttempt,
    redial: bool,
}

struct RouteEnd {
    dial: RouteDial,
    attached: bool,
    link_nonce: Option<String>,
}

struct RouteTask {
    route: CloudRoute,
    epoch: u64,
    attempts: watch::Sender<RouteDial>,
    end: Arc<parking_lot::Mutex<Option<RouteEnd>>>,
    task: JoinHandle<()>,
}

impl RouteTask {
    fn rearm(&self, attempt: RouteAttempt, link_nonce: impl FnOnce() -> Option<String>) {
        let Some(ended) = self.end.lock().take() else { return };
        let changed = ended.dial.attempt != attempt || ended.link_nonce != link_nonce();
        let recover = ended.attached
            || (!matches!(attempt.lifecycle, RouteLifecycle::LegacyWake { .. })
                && !ended.dial.redial);
        if changed || recover {
            self.attempts.send_replace(RouteDial {
                attempt,
                redial: !changed,
            });
        } else {
            *self.end.lock() = Some(ended);
        }
    }

    async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}

impl Drop for RouteTask {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn start(service: &ServiceState, mesh: MeshState) {
    struct Started;
    if service
        .try_ext::<codeterm_mesh::cloud_ingress::CloudIngressHandle>()
        .is_some()
        || service.set_ext(Started).is_err()
    {
        return;
    }
    let dispatcher = Arc::new(Dispatcher(Arc::downgrade(service)));
    let dedupe = Arc::new(tokio::sync::OnceCell::new());
    let route_mesh = mesh.clone();
    start_routes(service, mesh, move |target, route, timeout| {
        let mesh = route_mesh.clone();
        let dispatcher = Arc::clone(&dispatcher);
        let dedupe = Arc::clone(&dedupe);
        async move {
            let mut attached = false;
            let deadline = tokio::time::Instant::now() + timeout;
            let store = dedupe
                .get_or_try_init(|| async {
                    ReceiverDedupe::open_with_system_clock(
                        codeterm_service_core::paths::data_dir().join("mesh/cloud-receipts"),
                        DedupeLimits::default(),
                    )
                    .await
                    .map(Arc::new)
                })
                .await;
            let result = match store {
                Ok(store) => {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    log::debug!("[cloud] connecting reverse route for {target}");
                    maintain(
                        mesh,
                        target.clone(),
                        route,
                        store.clone(),
                        dispatcher,
                        remaining,
                        &mut attached,
                    )
                    .await
                }
                Err(_) => Err(IngressError::Unavailable),
            };
            if let Err(error) = result {
                log::debug!("[cloud] reverse route ended for {target}: {}", error.code());
            }
            attached
        }
    });
}

pub(crate) fn start_routes<Fut>(
    service: &ServiceState,
    mesh: MeshState,
    run: impl Fn(String, CloudRoute, Duration) -> Fut + Send + Sync + 'static,
) -> JoinHandle<()>
where
    Fut: Future<Output = bool> + Send + 'static,
{
    let service = Arc::downgrade(service);
    let run = Arc::new(run);
    tokio::spawn(async move {
        let mut tasks = HashMap::<String, RouteTask>::new();
        let mut prev_attached = HashSet::<String>::new();
        let mut gate = mesh.lock().gate_watch();
        let (ended, mut end_events) = watch::channel(0u64);
        let Some(mut cloud_lifecycle) = service
            .upgrade()
            .map(|owner| owner.cloud_machines().subscribe_lifecycle())
        else {
            return;
        };
        loop {
            let Some(owner) = service.upgrade() else {
                return;
            };
            cloud_lifecycle.borrow_and_update();
            end_events.borrow_and_update();
            let enabled = owner.configuration().mesh.enabled;
            let controller = owner.cloud_machines();
            let managed_records = controller
                .list_records()
                .into_iter()
                .map(|record| {
                    let confirmation = controller.wake_route_confirmation(&record.machine_id);
                    (record, confirmation)
                })
                .collect::<Vec<_>>();
            let mut managed_endpoints = HashMap::new();
            let (routes, bindings, attached) = {
                let mut manager = mesh.lock();
                for (record, confirmation) in managed_records {
                    let Some(inst_id) = record.installation_id else {
                        continue;
                    };
                    if record.state == CloudLifecycleState::Deleted {
                        manager.unpin_managed_cloud_endpoint(&inst_id);
                    } else {
                        if let Some(endpoint) = &record.endpoint {
                            manager.pin_managed_cloud_endpoint(inst_id.clone(), endpoint.clone());
                        }
                        let lifecycle = match record.state {
                            CloudLifecycleState::Waking => confirmation.map(|confirmed| {
                                let deadline_ms = confirmed.deadline_ms;
                                match confirmed.ingress_ready_at {
                                    Some(ingress_ready_at) => RouteLifecycle::Waking {
                                        deadline_ms,
                                        ingress_ready_at,
                                    },
                                    None => RouteLifecycle::LegacyWake { deadline_ms },
                                }
                            }),
                            CloudLifecycleState::Awake | CloudLifecycleState::Draining => {
                                Some(RouteLifecycle::Awake {
                                    since_ms: record.last_awake_at_ms,
                                })
                            }
                            CloudLifecycleState::Asleep | CloudLifecycleState::OutOfCredits => None,
                            _ => Some(RouteLifecycle::Unmanaged),
                        };
                        managed_endpoints
                            .insert(inst_id, (record.endpoint, record.state, lifecycle));
                    }
                }
                let epoch = manager.remote_access_epoch();
                let routes = manager
                    .endpoint_records()
                    .into_iter()
                    .filter_map(|r| {
                        if !enabled {
                            return None;
                        }
                        let managed = managed_endpoints.get(&r.claim.installation_id);
                        let route = managed
                            .and_then(|(endpoint, _, _)| endpoint.clone())
                            .or_else(|| r.claim.endpoint.and_then(|e| e.cloud_endpoint()))?;
                        let lifecycle = match managed {
                            Some((
                                _,
                                CloudLifecycleState::Asleep | CloudLifecycleState::OutOfCredits,
                                _,
                            )) => return None,
                            Some((_, _, lifecycle))
                                if matches!(route, CloudRoute::ProviderProxy { .. }) =>
                            {
                                (*lifecycle)?
                            }
                            _ => RouteLifecycle::Unmanaged,
                        };
                        let attempt = RouteAttempt {
                            sequence: r.claim.sequence,
                            lifecycle,
                        };
                        Some((r.claim.installation_id, (epoch?, route, attempt)))
                    })
                    .collect::<HashMap<_, _>>();
                let bindings = manager.installation_instance_bindings();
                let attached = manager.attached_reverse_streams();
                (routes, bindings, attached)
            };
            if !attached.is_empty() {
                controller.observe_verified_wake_routes(&attached);
            }
            let backfilled = owner.cloud_machines().observe_peer_instance_ids(
                bindings
                    .iter()
                    .map(|(inst, peer)| (inst.as_str(), peer.as_str())),
            );
            if attached != prev_attached || backfilled > 0 {
                prev_attached = attached;
                owner.ui.emit("mesh-state-changed", serde_json::Value::Null);
            }
            drop(owner);
            let obsolete = tasks
                .iter()
                .filter(|(id, task)| {
                    !routes.get(*id).is_some_and(|(epoch, route, _)| {
                        task.epoch == *epoch && task.route == *route && !task.task.is_finished()
                    })
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in obsolete {
                if let Some(task) = tasks.remove(&id) {
                    task.stop().await;
                }
            }
            for (id, (epoch, route, attempt)) in routes {
                if let Some(task) = tasks.get(&id) {
                    task.rearm(attempt, || current_link_nonce(&mesh.lock(), &id));
                    continue;
                }
                let target = id.clone();
                let run = Arc::clone(&run);
                let service = service.clone();
                let mesh = mesh.clone();
                let ended = ended.clone();
                let task_route = route.clone();
                let (attempts, mut changes) = watch::channel(RouteDial {
                    attempt,
                    redial: false,
                });
                let end = Arc::new(parking_lot::Mutex::new(None));
                let task_end = end.clone();
                let task = tokio::spawn(async move {
                    loop {
                        let dial = *changes.borrow_and_update();
                        let timeout = match dial.attempt.lifecycle {
                            RouteLifecycle::Waking { deadline_ms, .. }
                            | RouteLifecycle::LegacyWake { deadline_ms } => {
                                let Some(owner) = service.upgrade() else {
                                    return;
                                };
                                Duration::from_millis(
                                    deadline_ms.saturating_sub(owner.cloud_machines().now_ms()),
                                )
                            }
                            _ => Duration::from_secs(30),
                        };
                        let attached = if timeout.is_zero() {
                            false
                        } else {
                            run(target.clone(), task_route.clone(), timeout).await
                        };
                        let link_nonce = current_link_nonce(&mesh.lock(), &target);
                        *task_end.lock() = Some(RouteEnd {
                            dial,
                            attached,
                            link_nonce,
                        });
                        ended.send_modify(|generation| *generation = generation.wrapping_add(1));
                        if changes.changed().await.is_err() {
                            return;
                        }
                    }
                });
                tasks.insert(
                    id,
                    RouteTask {
                        route,
                        epoch,
                        attempts,
                        end,
                        task,
                    },
                );
            }
            tokio::select! {
                changed = gate.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                changed = cloud_lifecycle.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                changed = end_events.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
        }
    })
}

fn current_link_nonce(manager: &MeshManager, installation_id: &str) -> Option<String> {
    let (_, peer) = manager
        .installation_instance_bindings()
        .into_iter()
        .find(|(id, _)| id == installation_id)?;
    manager.link_record(&peer)?.link_nonce.clone()
}

#[cfg(test)]
mod tests;
