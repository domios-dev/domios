use axum::body::{to_bytes, Body};
use axum::http::{header, Request};
use codeterm_mesh::account_mesh::BoxFuture;
use codeterm_mesh::cloud_ingress::{connect_enrolled, CloudRoute, IngressError, ReverseDispatcher};
use codeterm_mesh::cloud_outbox::{DedupeLimits, OuterWireRequest, ReceiverDedupe};
use codeterm_mesh::endpoint_gossip::EndpointAddress;
use codeterm_mesh::MeshState;
use codeterm_service_core::{
    ports::rest::LocalApiRouter,
    service::{ServiceContext, ServiceState},
};
use std::{
    collections::HashMap,
    net::SocketAddrV4,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::task::JoinHandle;
use tower::ServiceExt;

struct Dispatcher(Weak<ServiceContext>);
pub fn dispatcher(service: &ServiceState) -> Arc<dyn ReverseDispatcher> {
    Arc::new(Dispatcher(Arc::downgrade(service)))
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
    address: SocketAddrV4,
    dedupe: Arc<ReceiverDedupe>,
    dispatcher: Arc<Dispatcher>,
) -> Result<(), IngressError> {
    let route = CloudRoute::Direct(address);
    let mut previous_expiry = None;
    loop {
        let channel = connect_enrolled(mesh.clone(), route, &id).await?;
        let member = channel.peer_membership()?;
        let expiry = member.expires_at;
        if previous_expiry.is_some_and(|prior| expiry <= prior) {
            return Err(IngressError::Rejected);
        }
        let refresh = expiry.saturating_sub(expiry.saturating_sub(member.issued_at) / 12);
        let delay = Duration::from_secs(refresh.saturating_sub(mesh.lock().now()));
        tokio::select! {
            result=channel.receive_reverse(&dedupe,dispatcher.as_ref())=>return result,
            _=tokio::time::sleep(delay)=>{}
        }
        connect_enrolled(mesh.clone(), route, &id)
            .await?
            .renew()
            .await?;
        connect_enrolled(mesh.clone(), route, &id)
            .await?
            .publish_endpoint(address)
            .await?;
        previous_expiry = Some(expiry);
    }
}

struct RouteTask {
    address: SocketAddrV4,
    epoch: u64,
    sequence: u64,
    task: JoinHandle<()>,
}
impl Drop for RouteTask {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn start(service: &ServiceState, mesh: MeshState) {
    if service
        .try_ext::<codeterm_mesh::cloud_ingress::CloudIngressHandle>()
        .is_some()
    {
        return;
    }
    let service = Arc::downgrade(service);
    tokio::spawn(async move {
        let dispatcher = Arc::new(Dispatcher(service.clone()));
        let mut tasks = HashMap::<String, RouteTask>::new();
        let mut gate = mesh.lock().gate_watch();
        let dedupe = tokio::sync::OnceCell::new();
        loop {
            let Some(owner) = service.upgrade() else {
                return;
            };
            let enabled = owner.configuration().mesh.enabled;
            let routes = {
                let manager = mesh.lock();
                let epoch = manager.remote_access_epoch();
                manager
                    .endpoint_records()
                    .into_iter()
                    .filter_map(|r| {
                        if !enabled {
                            return None;
                        }
                        match r.claim.endpoint {
                            Some(EndpointAddress::DirectIngress { address }) => {
                                Some((r.claim.installation_id, (epoch?, r.claim.sequence, address)))
                            }
                            _ => None,
                        }
                    })
                    .collect::<HashMap<_, _>>()
            };
            drop(owner);
            tasks.retain(|id, task| {
                routes.get(id).is_some_and(|(epoch, sequence, address)| {
                    task.epoch == *epoch
                        && task.address == *address
                        && (task.sequence == *sequence || !task.task.is_finished())
                })
            });
            for (id, (epoch, sequence, address)) in routes {
                if tasks.contains_key(&id) {
                    continue;
                }
                let Ok(store) = dedupe
                    .get_or_try_init(|| async {
                        ReceiverDedupe::open_with_system_clock(
                            codeterm_service_core::paths::data_dir().join("mesh/cloud-receipts"),
                            DedupeLimits::default(),
                        )
                        .await
                        .map(Arc::new)
                    })
                    .await
                else {
                    return;
                };
                let mesh = mesh.clone();
                let target = id.clone();
                let store = Arc::clone(store);
                let dispatcher = Arc::clone(&dispatcher);
                let task = tokio::spawn(async move {
                    let _ = maintain(mesh, target, address, store, dispatcher).await;
                });
                tasks.insert(
                    id,
                    RouteTask {
                        address,
                        epoch,
                        sequence,
                        task,
                    },
                );
            }
            if gate.changed().await.is_err() {
                return;
            }
        }
    });
}
