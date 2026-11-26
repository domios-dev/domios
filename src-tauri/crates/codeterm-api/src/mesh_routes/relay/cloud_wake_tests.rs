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
use codeterm_mesh::{account_mesh::BoxFuture, cloud_ingress::CloudRoute};
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

use crate::{
    cloud_routes::{
        wake_machine_for_use, CloudEndpointPublisherOverride, CloudProvisioningOverride,
        MeshPeerControlAdapter,
    },
    mesh_routes::relay::{
        CloudIngressConnector, CloudIngressConnectorOverride, CloudIngressSendError,
    },
    routes::{self, AppState},
};

#[derive(Clone)]
struct RecordedCall {
    command: RelayCommand,
    signed_token: String,
}

type ReplyFn =
    Box<dyn Fn(&RelayCommand) -> Result<RelayResponse, CloudIngressSendError> + Send + Sync>;

struct FakeCloudChannel {
    calls: Mutex<Vec<RecordedCall>>,
    replies: Mutex<VecDeque<ReplyFn>>,
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

    fn recorded(&self) -> Vec<RecordedCall> {
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
        _route: CloudRoute,
        _installation_id: String,
        command: RelayCommand,
        signed_token: String,
    ) -> BoxFuture<'_, Result<RelayResponse, CloudIngressSendError>> {
        self.calls.lock().push(RecordedCall {
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

fn setup_sleeping_cloud_peer(
    fixture: &ServiceFixture,
    instance_id: &str,
    cloud_addr: SocketAddrV4,
) -> (
    AppState,
    String,
    tempfile::TempDir,
    Arc<CloudMachineController>,
    Arc<FakeWakeProvisioning>,
    Arc<FakeCloudChannel>,
) {
    let state = crate::test_support::app_state(fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (key, public) = crown::generate_peer_keypair();
    let (_, cloud_pub) = crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.clear();
        mesh.ident.identity.instance_id = instance_id.into();
        mesh.ident.identity.private_key = Some(key);
        mesh.ident.identity.public_key = Some(public.clone());
        mesh.insert_account_peer_with_installation_for_test(
            "cloud-inst-wake".into(),
            PairedPeer {
                instance_id: "cloud-peer-wake".into(),
                instance_name: "cloud-sleeper".into(),
                addresses: Vec::new(),
                api_port: 0,
                public_key: cloud_pub,
                paired_at: "now".into(),
                last_seen: None,
                online: false,
                is_primary: false,
                access_level: AccessLevel::Full,
                domios_version: None,
                domios_build: None,
                web_ui: None,
                relay_protocol_version: RELAY_PROTOCOL_VERSION,
                consecutive_misses: 0,
                last_active_ms: None,
            },
        );
        mesh.insert_direct_ingress_endpoint_for_test("cloud-inst-wake".into(), cloud_addr);
    }

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
                controller_id: instance_id.into(),
                labels: BTreeMap::new(),
            },
        }])
        .unwrap();
    let controller = Arc::new(CloudMachineController::new(
        store,
        instance_id.into(),
        Arc::new(SystemClock),
    ));
    state
        .svc
        .install_cloud_machines_for_test(controller.clone());

    let provisioning = FakeWakeProvisioning::new(cloud_addr);
    assert!(state
        .svc
        .set_ext(CloudProvisioningOverride(provisioning.clone()))
        .is_ok());

    let channel = FakeCloudChannel::new();
    assert!(state
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    (state, public, store_dir, controller, provisioning, channel)
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
async fn agent_held_send_preserves_agent_origin_and_refuses_spoofed_source_tab_before_holding() {
    let (mux, agent_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let cloud_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 30), 7687);
    let (home, home_pub, _dir, controller, provisioning, channel) =
        setup_sleeping_cloud_peer(&fixture, "home-agent-wake", cloud_addr);

    let agent_token = home
        .svc
        .frontend_pty_env(Some(&agent_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = routes::create_router_with_frontend(home.clone(), None);

    // Spoofed source_tab from an agent tab must be refused BEFORE holding or waking.
    let (spoof_status, _) = post_relay_to(
        app.clone(),
        &agent_token,
        &json!({
            "target_peer_id": "cloud-sleeper",
            "method": "POST",
            "path": "/api/tabs/cloud-tab-1/send",
            "body": { "text": "spoofed", "source_tab": "other-tab-id" },
        }),
    )
    .await;
    assert_eq!(spoof_status, StatusCode::FORBIDDEN);
    assert_eq!(controller.held_send_count(), 0);
    assert_eq!(provisioning.wakes.load(Ordering::SeqCst), 0);
    assert!(channel.recorded().is_empty());

    // Legitimate agent send is held, wakes, and flushes signed as that Agent, never as Mesh/Owner.
    let (status, held_resp) = post_relay_to(
        app,
        &agent_token,
        &json!({
            "target_peer_id": "cloud-sleeper",
            "method": "POST",
            "path": "/api/tabs/cloud-tab-1/send",
            "body": { "text": "agent-opaque-wake" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{held_resp}");
    assert_eq!(held_resp["status"], 202, "{held_resp}");
    assert_eq!(held_resp["data"]["held"], true);
    assert_eq!(held_resp["data"]["bytes_written"], false);
    assert!(channel.recorded().is_empty());

    provisioning.release_wake();
    channel.wait_for_calls(1).await;

    let delivered = channel.recorded();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].command.body["source_tab"], agent_tab);

    let keys = [("home-agent-wake".to_owned(), home_pub)].into();
    let claims = crown::verify_request_claims(&delivered[0].signed_token, &keys)
        .expect("valid signed claims on flushed send");
    assert_eq!(
        claims.origin,
        AuthenticatedOrigin::Agent,
        "held agent send must arrive as Agent, never escalated to Mesh or Owner"
    );
    assert_eq!(
        claims.agent,
        Some(AuthenticatedAgent {
            instance_id: "home-agent-wake".into(),
            tab_id: agent_tab,
            can_spawn: false,
        })
    );
}

#[tokio::test]
async fn transient_connect_unavailable_keeps_held_send_pending_and_publish_failure_blocks_release()
{
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let cloud_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 31), 7687);
    let (home, _home_pub, _dir, controller, provisioning, channel) =
        setup_sleeping_cloud_peer(&fixture, "home-transient-wake", cloud_addr);

    // Override endpoint publication to fail once, then succeed and record the published address.
    let published = Arc::new(Mutex::new(Vec::<(SocketAddrV4, String)>::new()));
    let fail_once = Arc::new(AtomicBool::new(true));
    let published_clone = Arc::clone(&published);
    let fail_once_clone = Arc::clone(&fail_once);
    assert!(home
        .svc
        .set_ext(CloudEndpointPublisherOverride(Arc::new(move |addr, id| {
            let published = Arc::clone(&published_clone);
            let fail_once = Arc::clone(&fail_once_clone);
            Box::pin(async move {
                if fail_once.swap(false, Ordering::SeqCst) {
                    return Err("publish_direct_endpoint refused".into());
                }
                published.lock().push((addr, id));
                Ok(())
            })
        })))
        .is_ok());

    let held = controller
        .hold_send_for_wake(
            "cloud-sleeper-1",
            "cloud-tab-1",
            "deliv-transient-1".into(),
            "POST".into(),
            "/api/tabs/cloud-tab-1/send".into(),
            json!({ "text": "retry-after-connect-unavailable" }),
            AuthenticatedOrigin::Owner,
            None,
            60_000,
        )
        .unwrap();

    // First wake fails during publish_direct_endpoint: machine stays not ready and releases 0 sends.
    provisioning.release_wake();
    let err = wake_machine_for_use(&home, "cloud-sleeper-1")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "provider_unavailable");
    assert!(channel.recorded().is_empty());
    assert_eq!(
        controller.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Pending
    );

    // Second wake publishes the endpoint, then encounters transient ConnectUnavailable during flush.
    channel.push_reply(|_| {
        Err(CloudIngressSendError::ConnectUnavailable(
            "ingress listener still booting".into(),
        ))
    });
    wake_machine_for_use(&home, "cloud-sleeper-1")
        .await
        .unwrap();
    assert_eq!(
        published.lock().as_slice(),
        &[(cloud_addr, "cloud-inst-wake".to_owned())]
    );
    assert_eq!(channel.recorded().len(), 1);
    assert_eq!(
        controller.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Pending,
        "transient ConnectUnavailable must keep send Held, not Failed"
    );

    // Subsequent flush succeeds and delivers the held send once.
    let peer_ctrl = MeshPeerControlAdapter::new(home.clone());
    controller
        .flush_held_sends("cloud-sleeper-1", &peer_ctrl)
        .await;
    assert_eq!(channel.recorded().len(), 2);
    assert_eq!(
        controller.inspect_held_send(&held.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
