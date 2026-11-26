//! Registered-router tests for the cloud relay producer:
//! - Home-to-cloud dispatch over Ferry's enrolled ingress channel after all relay guards
//! - Cloud-to-home queueing through `CloudOutboxPort::enqueue` with `delivery_unconfirmed` and no replay
//! - Cloud wake held send delivered once over the ingress channel after wake
//! - Typed refusal for cloud-relayed notifications pending security review

use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_api::{
    cloud_mesh_runtime::{dispatcher, drain_policy},
    cloud_routes::{CloudProvisioningOverride, MeshPeerControlAdapter},
    mesh_routes::relay::{
        CloudIngressConnector, CloudIngressConnectorOverride, CloudIngressSendError,
        CloudOutboxOverride, CLOUD_NOTIFICATION_RELAY_REFUSED,
    },
    routes::{self, AppState},
};
use codeterm_mesh::{
    account_mesh::BoxFuture,
    cloud_ingress::{CloudRoute, IngressError, ReverseDispatcher},
    cloud_outbox::{
        CallerContext, CloudOutbox, CloudOutboxConfig, CloudOutboxPort, DrainAuthorizationPolicy,
        DrainPolicyDecision, OutboundOperation, OutboundPeerRequest, OutboxFrame, OutboxId,
        OutboxLimits, OuterWireRequest, ReverseStreamSink, StreamDeliveryOutcome,
        UnconfirmedReason, DELIVERY_UNCONFIRMED, OUTBOX_FULL, OUTER_RELAY_ROUTE,
        WORKER_ACK_RPC_METHOD, WORKER_REPORT_RPC_METHOD,
    },
    transport::{TransportError, TransportErrorKind},
};
use codeterm_service_core::{
    mesh::{
        crown::{self, AuthenticatedAgent, AuthenticatedOrigin},
        types::{AccessLevel, PairedPeer, RelayCommand, RelayResponse, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
    service::{
        chat_send_transaction::ChatSendState,
        cloud_machines::{
            CloudError, CloudLifecycleState, CloudMachineController, CloudMachineRecord,
            CloudMachineStore, CloudOwnershipReceipt, CloudProvisioningPort, CloudRate,
            CloudSleepMode, CloudUsageSummary, ProvisionedLease, ProvisionedMachine,
            ProvisioningCreateRequest, SystemClock,
        },
        test_fixture::ServiceFixture,
    },
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::Notify;
use tower::ServiceExt;

#[path = "../../src/test_support.rs"]
mod test_support;

mod security_fixes;

#[derive(Clone)]
struct RecordedChannelCall {
    route: CloudRoute,
    installation_id: String,
    command: RelayCommand,
    signed_token: String,
}

type ChannelReplyFactory =
    Box<dyn Fn(&RelayCommand) -> Result<RelayResponse, CloudIngressSendError> + Send + Sync>;

struct FakeCloudChannel {
    calls: Mutex<Vec<RecordedChannelCall>>,
    replies: Mutex<VecDeque<ChannelReplyFactory>>,
    calls_notify: Arc<Notify>,
}

impl FakeCloudChannel {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            replies: Mutex::new(VecDeque::new()),
            calls_notify: Arc::new(Notify::new()),
        })
    }

    fn push_reply<F>(&self, reply: F)
    where
        F: Fn(&RelayCommand) -> Result<RelayResponse, CloudIngressSendError>
            + Send
            + Sync
            + 'static,
    {
        self.replies.lock().push_back(Box::new(reply));
    }

    fn recorded(&self) -> Vec<RecordedChannelCall> {
        self.calls.lock().clone()
    }

    async fn wait_for_calls(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.calls_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.calls.lock().len() >= expected {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("timed out waiting for fake cloud channel call");
    }
}

impl CloudIngressConnector for FakeCloudChannel {
    fn send_relay(
        &self,
        _mesh: codeterm_mesh::MeshState,
        route: CloudRoute,
        installation_id: String,
        command: RelayCommand,
        signed_token: String,
    ) -> BoxFuture<'_, Result<RelayResponse, CloudIngressSendError>> {
        self.calls.lock().push(RecordedChannelCall {
            route,
            installation_id,
            command: command.clone(),
            signed_token,
        });
        let reply = self.replies.lock().pop_front().map(|f| f(&command));
        let notify = Arc::clone(&self.calls_notify);
        Box::pin(async move {
            notify.notify_waiters();
            reply.unwrap_or_else(|| {
                Ok(RelayResponse {
                    ok: true,
                    status: 200,
                    data: Some(json!({ "ok": true, "delivery": "sent" })),
                    error: None,
                })
            })
        })
    }
}

struct ScriptedReverseSink {
    stream_id: u64,
    account_id: String,
    local_id: String,
    remote_id: String,
    delivered: Mutex<Vec<OutboxFrame>>,
    outcome: Mutex<VecDeque<StreamDeliveryOutcome>>,
}

impl ScriptedReverseSink {
    fn new(
        stream_id: u64,
        account_id: &str,
        local_id: &str,
        remote_id: &str,
        outcomes: Vec<StreamDeliveryOutcome>,
    ) -> Arc<Self> {
        Arc::new(Self {
            stream_id,
            account_id: account_id.into(),
            local_id: local_id.into(),
            remote_id: remote_id.into(),
            delivered: Mutex::new(Vec::new()),
            outcome: Mutex::new(outcomes.into()),
        })
    }

    fn frames(&self) -> Vec<OutboxFrame> {
        self.delivered.lock().clone()
    }
}

impl ReverseStreamSink for ScriptedReverseSink {
    fn stream_id(&self) -> u64 {
        self.stream_id
    }
    fn account_id(&self) -> &str {
        &self.account_id
    }
    fn local_installation_id(&self) -> &str {
        &self.local_id
    }
    fn peer_installation_id(&self) -> &str {
        &self.remote_id
    }
    fn deliver<'a>(&'a self, frame: &'a OutboxFrame) -> BoxFuture<'a, StreamDeliveryOutcome> {
        self.delivered.lock().push(frame.clone());
        let outcome = self
            .outcome
            .lock()
            .pop_front()
            .unwrap_or_else(|| StreamDeliveryOutcome::acked(None));
        Box::pin(async move { outcome })
    }
}

struct FakeWakeProvisioning {
    endpoint: SocketAddrV4,
    wakes: AtomicUsize,
    allow_wake: AtomicBool,
    wake_gate: Notify,
}

impl FakeWakeProvisioning {
    fn new(endpoint: SocketAddrV4) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            wakes: AtomicUsize::new(0),
            allow_wake: AtomicBool::new(false),
            wake_gate: Notify::new(),
        })
    }

    fn release_wake(&self) {
        self.allow_wake.store(true, Ordering::SeqCst);
        self.wake_gate.notify_waiters();
    }
}

#[async_trait]
impl CloudProvisioningPort for FakeWakeProvisioning {
    async fn create(
        &self,
        _: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused in wake test".into()))
    }

    async fn wake(
        &self,
        machine_id: &str,
        _request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError> {
        loop {
            let notified = self.wake_gate.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.allow_wake.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
        self.wakes.fetch_add(1, Ordering::SeqCst);
        Ok(ProvisionedMachine {
            machine_id: machine_id.to_owned(),
            provider: "managed".into(),
            endpoint: self.endpoint,
            size: "small".into(),
            rate: CloudRate {
                amount: 0.05,
                currency: "USD".into(),
                credits_per_hour: Some(0.05),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-wake-1".into(),
            lease_expires_at_ms: 9_999_999_999_000,
        })
    }

    async fn sleep(&self, _: &str, _: &str) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::Stop)
    }

    async fn delete(&self, _: &str, _: &str) -> Result<(), CloudError> {
        Ok(())
    }

    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        _: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        Ok(ProvisionedLease {
            machine_id: machine_id.to_owned(),
            lease_id: lease_id.to_owned(),
            lease_expires_at_ms: 9_999_999_999_000,
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }
}

fn configure_peer_state(fixture: &ServiceFixture, instance_id: &str) -> (AppState, String) {
    let state = test_support::app_state(fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (key, public) = crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.clear();
        mesh.ident.identity.instance_id = instance_id.into();
        mesh.ident.identity.private_key = Some(key);
        mesh.ident.identity.public_key = Some(public.clone());
    }
    (state, public)
}

fn test_paired_peer(instance_id: &str, instance_name: &str) -> PairedPeer {
    let (_, public_key) = crown::generate_peer_keypair();
    PairedPeer {
        instance_id: instance_id.into(),
        instance_name: instance_name.into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        // Matches endpoint_gossip::receive_endpoint_record for DirectIngress peers.
        online: false,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

fn enroll_cloud_peer(
    home: &AppState,
    cloud_instance_id: &str,
    cloud_name: &str,
    cloud_installation_id: &str,
    address: SocketAddrV4,
) {
    let mut mesh = home.mesh.lock();
    mesh.insert_account_peer_with_installation_for_test(
        cloud_installation_id.into(),
        test_paired_peer(cloud_instance_id, cloud_name),
    );
    mesh.insert_direct_ingress_endpoint_for_test(cloud_installation_id.into(), address);
}

async fn post_relay_to(app: axum::Router, token: &str, payload: &Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/mesh/relay-to")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
    )
}

#[tokio::test]
async fn home_to_cloud_relay_travels_over_ingress_channel_after_all_relay_guards() {
    let (mux, local_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (home, home_pub) = configure_peer_state(&fixture, "home-inst");
    let cloud_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 10), 7687);
    let managed_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 22), 7687);
    // Register managed-only peer (persisted endpoint_addr, no gossip DirectIngress) before
    // enroll_cloud_peer populates registry_members and before cloud_machines() initializes.
    home.mesh
        .lock()
        .insert_account_peer_with_installation_for_test(
            "cloud-inst-managed".into(),
            test_paired_peer("cloud-peer-managed", "cloud-managed"),
        );
    let managed_dir = tempfile::tempdir().unwrap();
    let managed_store = CloudMachineStore::new(managed_dir.path().join("managed.json"));
    managed_store
        .save(&[CloudMachineRecord {
            machine_id: "cloud-managed-1".into(),
            name: "cloud-managed".into(),
            provider: "managed".into(),
            size: "small".into(),
            state: CloudLifecycleState::Awake,
            sleep_mode: CloudSleepMode::Stop,
            installation_id: Some("cloud-inst-managed".into()),
            peer_instance_id: Some("cloud-peer-managed".into()),
            endpoint_addr: Some(managed_addr),
            rate_per_hour: None,
            created_at_ms: 1_000,
            last_awake_at_ms: Some(1_000),
            accumulated_awake_secs: 0,
            period_start_ms: 1_000,
            lease_id: None,
            lease_expires_at_ms: None,
            exhaustion_pending: false,
            idle_since_ms: None,
            apply_unconfirmed: false,
            pending_request: None,
            last_error: None,
            ownership: CloudOwnershipReceipt::default(),
        }])
        .unwrap();
    home.svc
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            managed_store,
            "home-inst".into(),
            Arc::new(SystemClock),
        )));
    enroll_cloud_peer(
        &home,
        "cloud-peer-1",
        "cloud-worker",
        "cloud-inst-1",
        cloud_addr,
    );
    assert!(
        home.mesh
            .lock()
            .peer_route_endpoints("cloud-peer-1")
            .is_none(),
        "enrolled cloud peer has no HTTP PeerClient route"
    );

    let channel = FakeCloudChannel::new();
    assert!(home
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(home.clone(), None);
    let tab_token = home
        .svc
        .frontend_pty_env(Some(&local_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    // 1. Caller binding guard runs BEFORE the channel: spoofed source_tab is refused.
    let (status, _) = post_relay_to(
        app.clone(),
        &tab_token,
        &json!({
            "target_peer_id": "cloud-worker",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "hi", "source_tab": "forged-tab" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        channel.recorded().is_empty(),
        "refused request must not touch channel"
    );

    // 2. Authorized tab send crosses over the fake ingress channel with caller binding,
    // delivery_id, and raw peer-signed token (no Bearer prefix).
    let (status, resp) = post_relay_to(
        app.clone(),
        &tab_token,
        &json!({
            "target_peer_id": "cloud-worker",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "hello cloud" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["peer"]["instance_id"], "cloud-peer-1");

    let recorded = channel.recorded();
    assert_eq!(recorded.len(), 1);
    let CloudRoute::Direct(sent_addr) = recorded[0].route;
    assert_eq!(sent_addr, cloud_addr);
    assert_eq!(recorded[0].installation_id, "cloud-inst-1");
    assert_eq!(recorded[0].command.method, "POST");
    assert_eq!(recorded[0].command.path, "/api/tabs/remote-tab/send");
    assert_eq!(recorded[0].command.body["source_tab"], local_tab);
    let delivery_id = recorded[0]
        .command
        .delivery_id
        .as_deref()
        .expect("tab send carries delivery_id");
    assert!(uuid::Uuid::parse_str(delivery_id).is_ok());
    assert!(
        !recorded[0].signed_token.starts_with("Bearer "),
        "send_relay receives raw signed token without Bearer prefix"
    );
    let keys = [("home-inst".to_owned(), home_pub.clone())].into();
    let claims = crown::verify_request_claims(&recorded[0].signed_token, &keys).unwrap();
    assert_eq!(claims.origin, AuthenticatedOrigin::Agent);
    assert_eq!(claims.agent.as_ref().unwrap().tab_id, local_tab);

    // 3. Spawn enrichment runs BEFORE the channel, and owner origin is preserved.
    channel.push_reply(|_| {
        Ok(RelayResponse {
            ok: true,
            status: 200,
            data: Some(json!({ "tab_id": "remote-worker-tab", "title": "RelayWorker" })),
            error: None,
        })
    });
    let (status, resp) = post_relay_to(
        app.clone(),
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-worker",
            "method": "POST",
            "path": "/api/agents/spawn",
            "body": {
                "agent": "claude",
                "spawned_by_tab_id": local_tab,
                "report_to": local_tab,
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], true, "{resp}");
    let spawn_call = channel.recorded().pop().unwrap();
    assert_eq!(
        spawn_call.command.body["ownership"]["controller_tab_id"],
        local_tab
    );
    let spawn_claims = crown::verify_request_claims(&spawn_call.signed_token, &keys).unwrap();
    assert_eq!(spawn_claims.origin, AuthenticatedOrigin::Owner);
    assert!(spawn_claims.agent.is_none());

    // 4. Pre-write admission failure surfaces bytes_written=false; lost reply surfaces
    // SubmitUnconfirmed with no replay.
    channel.push_reply(|_| Err(CloudIngressSendError::from_ingress(IngressError::Rejected)));
    let (status, rejected) = post_relay_to(
        app.clone(),
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-worker",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "rejected before write" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["status"], 403);
    assert_eq!(rejected["data"]["error"]["bytes_written"], false);

    let before_lost = channel.recorded().len();
    channel.push_reply(|_| {
        Err(CloudIngressSendError::Transport(
            TransportError::new_for_test(
                TransportErrorKind::SubmitUnconfirmed,
                "cloud relay receipt unavailable",
            ),
        ))
    });
    let (status, unconfirmed) = post_relay_to(
        app.clone(),
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-worker",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "lost reply" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unconfirmed["ok"], false);
    assert_eq!(unconfirmed["status"], 504);
    assert_eq!(
        unconfirmed["data"]["error"]["code"],
        codeterm_mesh::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
    );
    assert!(
        unconfirmed["data"]["error"].get("bytes_written").is_none(),
        "SubmitUnconfirmed never claims bytes_written=false: {unconfirmed}"
    );
    assert_eq!(
        channel.recorded().len(),
        before_lost + 1,
        "SubmitUnconfirmed must never replay"
    );

    // 5. Persisted managed endpoint_addr fallback works when gossip has no DirectIngress record.
    assert!(home
        .mesh
        .lock()
        .direct_ingress_address("cloud-inst-managed")
        .is_none());
    let (status, managed_resp) = post_relay_to(
        app,
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-managed",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "hello managed endpoint_addr" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{managed_resp}");
    assert_eq!(managed_resp["ok"], true, "{managed_resp}");
    let managed_call = channel.recorded().pop().unwrap();
    let CloudRoute::Direct(sent_managed) = managed_call.route;
    assert_eq!(sent_managed, managed_addr);
    assert_eq!(managed_call.installation_id, "cloud-inst-managed");
}

#[tokio::test]
async fn cloud_to_home_requests_enqueue_through_outbox_and_surface_delivery_unconfirmed_without_replay(
) {
    let (mux, worker_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (cloud, _) = configure_peer_state(&fixture, "cloud-inst");
    cloud
        .mesh
        .lock()
        .insert_account_peer_with_installation_for_test(
            "home-inst".into(),
            test_paired_peer("home-peer", "HomeMac"),
        );

    let outbox_dir = tempfile::tempdir().unwrap();
    let policy = drain_policy(&cloud.svc, cloud.mesh.clone());
    let outbox = Arc::new(
        CloudOutbox::open(
            outbox_dir.path().join("outbox"),
            CloudOutboxConfig {
                policy,
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert!(cloud
        .svc
        .set_ext(CloudOutboxOverride(outbox.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(cloud.clone(), None);
    let worker_token = cloud
        .svc
        .frontend_pty_env(Some(&worker_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    // 1. Attached reverse stream delivers WorkerAck and returns home's RelayResponse.
    let home_reply = serde_json::to_vec(&RelayResponse {
        ok: true,
        status: 200,
        data: Some(json!({ "result": { "acknowledged": true } })),
        error: None,
    })
    .unwrap();
    let sink_ok = ScriptedReverseSink::new(
        1,
        "test-account",
        "cloud-inst",
        "home-inst",
        vec![StreamDeliveryOutcome::acked(Some(home_reply))],
    );
    outbox.attach_reverse_stream(sink_ok.clone()).await;

    let (status, ack_resp) = post_relay_to(
        app.clone(),
        &worker_token,
        &json!({
            "target_peer_id": "HomeMac",
            "method": "POST",
            "path": "/api/mesh/rpc",
            "body": {
                "method": WORKER_ACK_RPC_METHOD,
                "request_id": "ack-1",
                "protocol_version": 1,
                "params": {
                    "plan_id": "pl_1",
                    "task_id": "task_1",
                    "attempt_id": "att_1",
                    "worker_tab_id": worker_tab,
                    "session_binding_nonce": "nonce-1"
                }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ack_resp}");
    assert_eq!(ack_resp["ok"], true, "{ack_resp}");
    assert_eq!(ack_resp["data"]["result"]["acknowledged"], true);
    assert_eq!(sink_ok.frames().len(), 1);
    assert_eq!(
        sink_ok.frames()[0].request.operation,
        OutboundOperation::WorkerAck
    );
    assert_eq!(
        sink_ok.frames()[0].request.caller_context.origin,
        AuthenticatedOrigin::Agent
    );
    assert_eq!(
        sink_ok.frames()[0]
            .request
            .caller_context
            .agent
            .as_ref()
            .unwrap()
            .tab_id,
        worker_tab
    );

    // 2. Lost ack on WorkerReport yields delivery_unconfirmed and NEVER replays on reconnect.
    let sink_lost = ScriptedReverseSink::new(
        2,
        "test-account",
        "cloud-inst",
        "home-inst",
        vec![StreamDeliveryOutcome::unconfirmed_write_completed(
            UnconfirmedReason::AckTimeout,
            Some("reverse ack timed out".into()),
        )],
    );
    outbox.attach_reverse_stream(sink_lost.clone()).await;

    let (status, unconfirmed) = post_relay_to(
        app.clone(),
        &worker_token,
        &json!({
            "target_peer_id": "HomeMac",
            "method": "POST",
            "path": "/api/mesh/rpc",
            "body": {
                "method": WORKER_REPORT_RPC_METHOD,
                "request_id": "report-1",
                "protocol_version": 1,
                "params": {
                    "plan_id": "pl_1",
                    "task_id": "task_1",
                    "attempt_id": "att_1",
                    "worker_tab_id": worker_tab,
                    "session_binding_nonce": "nonce-1",
                    "report": {
                        "id": "rep-1",
                        "message": "done",
                        "status": "in_review",
                        "reporter_tab_id": worker_tab
                    }
                }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unconfirmed}");
    assert_eq!(unconfirmed["ok"], false, "{unconfirmed}");
    assert_eq!(unconfirmed["status"], 504, "{unconfirmed}");
    assert_eq!(
        unconfirmed["data"]["error"]["code"], DELIVERY_UNCONFIRMED,
        "{unconfirmed}"
    );
    assert_eq!(unconfirmed["data"]["error"]["bytes_written"], true);
    let unconfirmed_id = unconfirmed["data"]["outbox_id"]
        .as_str()
        .expect("outbox_id surfaced")
        .to_owned();
    assert_eq!(sink_lost.frames().len(), 1);
    assert!(outbox
        .unconfirmed_records()
        .iter()
        .any(|(id, _)| id.as_str() == unconfirmed_id));

    // Reattaching a healthy stream drains new requests without replaying the unconfirmed report.
    let ack2_reply = serde_json::to_vec(&RelayResponse {
        ok: true,
        status: 200,
        data: Some(json!({ "result": { "ok": true } })),
        error: None,
    })
    .unwrap();
    let sink_reconnect = ScriptedReverseSink::new(
        3,
        "test-account",
        "cloud-inst",
        "home-inst",
        vec![StreamDeliveryOutcome::acked(Some(ack2_reply))],
    );
    outbox.attach_reverse_stream(sink_reconnect.clone()).await;

    let (status, ack2) = post_relay_to(
        app,
        &worker_token,
        &json!({
            "target_peer_id": "HomeMac",
            "method": "POST",
            "path": "/api/mesh/rpc",
            "body": {
                "method": WORKER_ACK_RPC_METHOD,
                "request_id": "ack-2",
                "protocol_version": 1,
                "params": {
                    "plan_id": "pl_1",
                    "task_id": "task_2",
                    "attempt_id": "att_2",
                    "worker_tab_id": worker_tab,
                    "session_binding_nonce": "nonce-2"
                }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ack2["ok"], true, "{ack2}");
    let reconnected_frames = sink_reconnect.frames();
    assert_eq!(
        reconnected_frames.len(),
        1,
        "lost-ack report must never replay on reconnect"
    );
    assert_eq!(
        reconnected_frames[0].request.operation,
        OutboundOperation::WorkerAck
    );

    // 3. Outbox full surfaces typed refusal with bytes_written=false.
    let small_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let (small_cloud, _) = configure_peer_state(&small_fixture, "small-cloud");
    small_cloud
        .mesh
        .lock()
        .insert_account_peer_with_installation_for_test(
            "home-inst".into(),
            test_paired_peer("home-peer", "HomeMac"),
        );
    let small_outbox = Arc::new(
        CloudOutbox::open(
            outbox_dir.path().join("small-outbox"),
            CloudOutboxConfig {
                limits: OutboxLimits {
                    max_records: 1,
                    ..Default::default()
                },
                policy: drain_policy(&small_cloud.svc, small_cloud.mesh.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert!(small_cloud
        .svc
        .set_ext(CloudOutboxOverride(small_outbox))
        .is_ok());
    let small_app = routes::create_router_with_frontend(small_cloud, None);
    let plan_rpc = |id: &str| {
        json!({
            "target_peer_id": "HomeMac",
            "method": "POST",
            "path": "/api/mesh/rpc",
            "body": {
                "method": "plan.get",
                "request_id": id,
                "protocol_version": 1,
                "params": { "plan_id": "pl_1" }
            }
        })
    };
    let (_, first) = post_relay_to(small_app.clone(), "owner-secret", &plan_rpc("p1")).await;
    assert_eq!(
        first["status"], 202,
        "first record queued while detached: {first}"
    );
    assert_eq!(first["data"]["queued"], true);
    let (_, second) = post_relay_to(small_app, "owner-secret", &plan_rpc("p2")).await;
    assert_eq!(second["ok"], false, "{second}");
    assert_eq!(second["status"], 503, "{second}");
    assert_eq!(second["data"]["error"]["code"], OUTBOX_FULL, "{second}");
    assert_eq!(second["data"]["error"]["bytes_written"], false);
}

#[tokio::test]
async fn cloud_wake_held_send_is_delivered_once_over_channel_after_wake() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let (home, home_pub) = configure_peer_state(&fixture, "home-wake");
    let cloud_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 11), 7687);
    enroll_cloud_peer(
        &home,
        "cloud-peer-wake",
        "cloud-sleeper",
        "cloud-inst-wake",
        cloud_addr,
    );

    let store_dir = tempfile::tempdir().unwrap();
    let store = CloudMachineStore::new(store_dir.path().join("cloud_machines.json"));
    store
        .save(&[CloudMachineRecord {
            machine_id: "cloud-sleeper-1".into(),
            name: "cloud-sleeper".into(),
            provider: "managed".into(),
            size: "small".into(),
            state: CloudLifecycleState::Asleep,
            sleep_mode: CloudSleepMode::Stop,
            installation_id: Some("cloud-inst-wake".into()),
            peer_instance_id: Some("cloud-peer-wake".into()),
            endpoint_addr: None,
            rate_per_hour: None,
            created_at_ms: 1_000,
            last_awake_at_ms: None,
            accumulated_awake_secs: 0,
            period_start_ms: 1_000,
            lease_id: None,
            lease_expires_at_ms: None,
            exhaustion_pending: false,
            idle_since_ms: None,
            apply_unconfirmed: false,
            pending_request: None,
            last_error: None,
            ownership: CloudOwnershipReceipt {
                provider_machine_id: "prov-1".into(),
                floating_ip_id: None,
                auxiliary_resource_ids: BTreeMap::new(),
                controller_id: "home-wake".into(),
                labels: BTreeMap::new(),
            },
        }])
        .unwrap();
    let controller = Arc::new(CloudMachineController::new(
        store,
        "home-wake".into(),
        Arc::new(SystemClock),
    ));
    home.svc.install_cloud_machines_for_test(controller.clone());

    let provisioning = FakeWakeProvisioning::new(cloud_addr);
    assert!(home
        .svc
        .set_ext(CloudProvisioningOverride(provisioning.clone()))
        .is_ok());

    let channel = FakeCloudChannel::new();
    assert!(home
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(home.clone(), None);
    let (status, held_resp) = post_relay_to(
        app,
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-sleeper",
            "method": "POST",
            "path": "/api/tabs/cloud-tab-1/send",
            "body": { "text": "wake and deliver once" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{held_resp}");
    assert_eq!(held_resp["ok"], true, "{held_resp}");
    assert_eq!(held_resp["status"], 202, "{held_resp}");
    assert_eq!(held_resp["data"]["held"], true);
    assert_eq!(held_resp["data"]["bytes_written"], false);
    let send_id = held_resp["data"]["send_id"].as_str().unwrap().to_owned();
    assert!(
        channel.recorded().is_empty(),
        "held send must not hit the channel while asleep"
    );

    // Release the managed provisioning wake started by preflight_cloud_wake.
    provisioning.release_wake();
    channel.wait_for_calls(1).await;

    let woken_record = controller.find_by_peer("cloud-sleeper-1").unwrap();
    assert_eq!(woken_record.state, CloudLifecycleState::Awake);
    assert_eq!(woken_record.endpoint_addr, Some(cloud_addr));
    assert_eq!(provisioning.wakes.load(Ordering::SeqCst), 1);

    let delivered = channel.recorded();
    assert_eq!(delivered.len(), 1, "held send delivered once after wake");
    let CloudRoute::Direct(addr) = delivered[0].route;
    assert_eq!(addr, cloud_addr);
    assert_eq!(delivered[0].installation_id, "cloud-inst-wake");
    assert_eq!(delivered[0].command.method, "POST");
    assert_eq!(delivered[0].command.path, "/api/tabs/cloud-tab-1/send");
    assert_eq!(delivered[0].command.body["text"], "wake and deliver once");
    assert!(delivered[0].command.delivery_id.is_some());
    assert!(!delivered[0].signed_token.starts_with("Bearer "));
    let keys = [("home-wake".to_owned(), home_pub)].into();
    assert!(crown::verify_request_claims(&delivered[0].signed_token, &keys).is_ok());

    assert_eq!(
        controller.inspect_held_send(&send_id).unwrap().state,
        ChatSendState::Sent
    );

    // Flushing again must never deliver a second time.
    let peer_ctrl = MeshPeerControlAdapter::new(home.clone());
    let second_flush = controller
        .flush_held_sends("cloud-sleeper-1", &peer_ctrl)
        .await;
    assert!(second_flush.is_empty());
    assert_eq!(channel.recorded().len(), 1);
}

#[tokio::test]
async fn cloud_relayed_notifications_are_refused_with_typed_error_not_widened() {
    let (mux, worker_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (cloud, _) = configure_peer_state(&fixture, "cloud-notif");
    cloud
        .mesh
        .lock()
        .insert_account_peer_with_installation_for_test(
            "home-inst".into(),
            test_paired_peer("home-peer", "HomeMac"),
        );

    let outbox_dir = tempfile::tempdir().unwrap();
    let policy = drain_policy(&cloud.svc, cloud.mesh.clone());
    let outbox = Arc::new(
        CloudOutbox::open(
            outbox_dir.path().join("notif-outbox"),
            CloudOutboxConfig {
                policy: policy.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert!(cloud
        .svc
        .set_ext(CloudOutboxOverride(outbox.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(cloud.clone(), None);
    let worker_token = cloud
        .svc
        .frontend_pty_env(Some(&worker_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    for token in ["owner-secret", worker_token.as_str()] {
        let (status, resp) = post_relay_to(
            app.clone(),
            token,
            &json!({
                "target_peer_id": "HomeMac",
                "method": "POST",
                "path": "/api/notifications",
                "body": { "title": "Cloud alert", "body": "ping" },
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{resp}");
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["status"], 403, "{resp}");
        assert_eq!(
            resp["data"]["error"]["code"], CLOUD_NOTIFICATION_RELAY_REFUSED,
            "{resp}"
        );
        assert_eq!(resp["data"]["error"]["bytes_written"], false);
    }
    assert_eq!(
        outbox.snapshot().queued_count,
        0,
        "refused notification must never enqueue"
    );

    // Live DrainAuthorizationPolicy also refuses Notification frames before write.
    let notif_req = OutboundPeerRequest {
        account_id: "test-account".into(),
        source_installation_id: "cloud-notif".into(),
        target_installation_id: "home-inst".into(),
        operation: OutboundOperation::Notification,
        caller_context: CallerContext {
            origin: AuthenticatedOrigin::Agent,
            agent: Some(AuthenticatedAgent {
                instance_id: "cloud-notif".into(),
                tab_id: worker_tab,
                can_spawn: false,
            }),
            expires_at_ms: None,
        },
        body: serde_json::to_vec(&json!({ "title": "Direct enqueue attempt" })).unwrap(),
    };
    let frame = OutboxFrame {
        id: OutboxId::generate(),
        sequence: 1,
        enqueued_at_ms: 1_000,
        payload_digest: notif_req.payload_digest(),
        request: notif_req,
    };
    assert_eq!(
        policy.authorize_drain(&frame, 1_500),
        DrainPolicyDecision::Refuse {
            reason: CLOUD_NOTIFICATION_RELAY_REFUSED.into(),
        }
    );

    // ReverseDispatcher on the home receiver also refuses relayed cloud notifications.
    let home_dispatcher = dispatcher(&cloud.svc);
    let wire = OuterWireRequest {
        route: OUTER_RELAY_ROUTE,
        body: serde_json::to_value(RelayCommand {
            method: "POST".into(),
            path: "/api/notifications".into(),
            body: json!({ "title": "Sneaky cloud bell" }),
            delivery_id: None,
        })
        .unwrap(),
    };
    assert_eq!(
        home_dispatcher
            .dispatch(wire, "unused-token".into())
            .await
            .unwrap_err(),
        IngressError::Rejected
    );
}
