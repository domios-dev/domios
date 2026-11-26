use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Notify;

use codeterm_mesh::account_mesh::ports::BoxFuture;
use codeterm_mesh::cloud_outbox::{
    CallerContext, CloudOutbox, CloudOutboxConfig, CloudOutboxPort, DedupeDecision, DedupeKey,
    DedupeLimits, DrainLifecycleObserver, OutboundOperation, OutboundPeerRequest, OutboxFrame,
    OutboxId, OutboxRecordStatus, ReceiverDedupe, ReverseStreamSink, StreamDeliveryOutcome,
    UnconfirmedReason, DELIVERY_UNCONFIRMED,
};
use codeterm_mesh::crown::AuthenticatedOrigin;

const ENV_SCENARIO: &str = "CODETERM_OUTBOX_CRASH_SCENARIO";
const ENV_OUTBOX_DIR: &str = "CODETERM_OUTBOX_CRASH_DIR";
const ENV_DEDUPE_DIR: &str = "CODETERM_OUTBOX_CRASH_DEDUPE_DIR";
const ENV_RECEIVER_ADDR: &str = "CODETERM_OUTBOX_CRASH_ADDR";

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create crash test temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn make_request(body: &[u8]) -> OutboundPeerRequest {
    OutboundPeerRequest {
        account_id: "acct-crash".into(),
        source_installation_id: "cloud-crash".into(),
        target_installation_id: "home-crash".into(),
        operation: OutboundOperation::WorkerReport,
        caller_context: CallerContext {
            origin: AuthenticatedOrigin::Agent,
            agent: None,
            expires_at_ms: None,
        },
        body: body.to_vec(),
    }
}

struct TcpReverseSink {
    stream_id: u64,
    addr: String,
}

impl ReverseStreamSink for TcpReverseSink {
    fn stream_id(&self) -> u64 {
        self.stream_id
    }

    fn account_id(&self) -> &str {
        "acct-crash"
    }

    fn local_installation_id(&self) -> &str {
        "cloud-crash"
    }

    fn peer_installation_id(&self) -> &str {
        "home-crash"
    }

    fn deliver<'a>(&'a self, frame: &'a OutboxFrame) -> BoxFuture<'a, StreamDeliveryOutcome> {
        let addr = self.addr.clone();
        let line = serde_json::to_string(frame).expect("serialize outbox frame");
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut stream = match TcpStream::connect(&addr) {
                    Ok(stream) => stream,
                    Err(error) => {
                        return StreamDeliveryOutcome::not_written(format!(
                            "connect failed: {error}"
                        ));
                    }
                };
                if let Err(error) = writeln!(stream, "{line}").and_then(|()| stream.flush()) {
                    return StreamDeliveryOutcome::unconfirmed_unknown(
                        UnconfirmedReason::WriteInterrupted,
                        Some(error.to_string()),
                    );
                }
                let mut reader = BufReader::new(stream);
                let mut reply = String::new();
                match reader.read_line(&mut reply) {
                    Ok(0) => StreamDeliveryOutcome::unconfirmed_write_completed(
                        UnconfirmedReason::StreamClosedBeforeAck,
                        Some("eof waiting for ack".into()),
                    ),
                    Ok(_) if reply.trim() == "ACK" => StreamDeliveryOutcome::acked(None),
                    Ok(other) => StreamDeliveryOutcome::unconfirmed_write_completed(
                        UnconfirmedReason::ReceiverStateUncertain,
                        Some(format!("unexpected ack ({other} bytes): {}", reply.trim())),
                    ),
                    Err(error) => StreamDeliveryOutcome::unconfirmed_write_completed(
                        UnconfirmedReason::StreamClosedBeforeAck,
                        Some(error.to_string()),
                    ),
                }
            })
            .await
            .unwrap_or_else(|error| {
                StreamDeliveryOutcome::unconfirmed_unknown(
                    UnconfirmedReason::TransportUncertain,
                    Some(error.to_string()),
                )
            })
        })
    }
}

struct ClaimPauseObserver;

impl DrainLifecycleObserver for ClaimPauseObserver {
    fn on_claimed_persisted(&self, _id: &OutboxId, _stream_id: u64) {
        println!("READY_CLAIMED");
        let _ = std::io::stdout().flush();
        let mut hold = String::new();
        let _ = std::io::stdin().read_line(&mut hold);
    }
}

struct SettledObserver {
    statuses: Mutex<HashMap<OutboxId, OutboxRecordStatus>>,
    notify: Arc<Notify>,
}

impl SettledObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            statuses: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
        })
    }

    async fn wait(&self, id: &OutboxId) -> OutboxRecordStatus {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(status) = self.statuses.lock().get(id).cloned() {
                return status;
            }
            tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .expect("timed out waiting for post-crash settlement");
        }
    }
}

impl DrainLifecycleObserver for SettledObserver {
    fn on_settled(&self, id: &OutboxId, status: &OutboxRecordStatus) {
        self.statuses.lock().insert(id.clone(), status.clone());
        self.notify.notify_waiters();
    }
}

fn spawn_crash_child(
    scenario: &str,
    outbox_dir: &Path,
    dedupe_dir: &Path,
    receiver_addr: Option<&str>,
) -> Child {
    let exe = std::env::current_exe().expect("resolve current test binary");
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", "crash_child_entrypoint", "--nocapture"])
        .env(ENV_SCENARIO, scenario)
        .env(ENV_OUTBOX_DIR, outbox_dir)
        .env(ENV_DEDUPE_DIR, dedupe_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(addr) = receiver_addr {
        cmd.env(ENV_RECEIVER_ADDR, addr);
    }
    cmd.spawn().expect("spawn crash child process")
}

fn wait_for_child_marker(child: &mut Child, marker: &str) {
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read child stdout");
        assert!(read > 0, "child exited before emitting marker {marker}");
        if line.trim() == marker {
            return;
        }
    }
}

fn start_mock_receiver(
    dedupe_dir: PathBuf,
    hold_ack_for_id: Option<String>,
) -> (
    String,
    Arc<Mutex<HashMap<String, usize>>>,
    Arc<Notify>,
    Arc<AtomicUsize>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock receiver");
    let addr = listener.local_addr().unwrap().to_string();
    let counts = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
    let hold_hit = Arc::new(Notify::new());
    let hold_flag = Arc::new(AtomicUsize::new(0));

    let counts_bg = Arc::clone(&counts);
    let hold_hit_bg = Arc::clone(&hold_hit);
    let hold_flag_bg = Arc::clone(&hold_flag);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("receiver runtime");
        let dedupe = rt
            .block_on(ReceiverDedupe::open_with_system_clock(
                &dedupe_dir,
                DedupeLimits::default(),
            ))
            .expect("open receiver dedupe");

        let mut held_streams = Vec::new();
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                break;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone tcp stream"));
            let mut line = String::new();
            let Ok(n) = reader.read_line(&mut line) else {
                continue;
            };
            if n == 0 {
                continue;
            }
            let frame: OutboxFrame = match serde_json::from_str(line.trim()) {
                Ok(frame) => frame,
                Err(_) => continue,
            };
            let key = frame.dedupe_key();
            let decision = rt
                .block_on(dedupe.reserve(key.clone(), frame.payload_digest.clone()))
                .expect("receiver dedupe reserve");

            let should_ack = match decision {
                DedupeDecision::Execute => {
                    *counts_bg
                        .lock()
                        .entry(frame.id.as_str().to_string())
                        .or_insert(0) += 1;
                    rt.block_on(dedupe.complete(&key, None))
                        .expect("receiver dedupe complete");
                    true
                }
                DedupeDecision::DuplicateCompleted { .. } => true,
                DedupeDecision::InFlightUncertain | DedupeDecision::CrashUncertain => false,
            };

            if hold_ack_for_id
                .as_deref()
                .is_some_and(|held| held == frame.id.as_str())
                && hold_flag_bg.fetch_add(1, Ordering::SeqCst) == 0
            {
                // Keep socket open without replying so sender remains InFlight until hard kill.
                held_streams.push(stream);
                hold_hit_bg.notify_waiters();
                continue;
            }

            if should_ack {
                let _ = writeln!(stream, "ACK");
                let _ = stream.flush();
            }
        }
    });

    (addr, counts, hold_hit, hold_flag)
}

#[test]
fn crash_child_entrypoint() {
    let Ok(scenario) = std::env::var(ENV_SCENARIO) else {
        return;
    };
    let outbox_dir = PathBuf::from(std::env::var(ENV_OUTBOX_DIR).expect("outbox dir"));
    let dedupe_dir = PathBuf::from(std::env::var(ENV_DEDUPE_DIR).expect("dedupe dir"));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("child tokio runtime");

    rt.block_on(async move {
        match scenario.as_str() {
            "enqueue_then_wait_kill" => {
                let outbox = CloudOutbox::open_default(&outbox_dir).await.unwrap();
                outbox
                    .enqueue_with_id(
                        OutboxId::parse("crash-rec-enqueue").unwrap(),
                        make_request(b"enqueue-payload"),
                    )
                    .await
                    .unwrap();
                println!("READY_ENQUEUED");
                let _ = std::io::stdout().flush();
                let mut hold = String::new();
                let _ = std::io::stdin().read_line(&mut hold);
            }
            "claim_before_wire_kill" => {
                let addr = std::env::var(ENV_RECEIVER_ADDR).expect("receiver addr");
                let outbox = CloudOutbox::open(
                    &outbox_dir,
                    CloudOutboxConfig {
                        observer: Arc::new(ClaimPauseObserver),
                        ..CloudOutboxConfig::default()
                    },
                )
                .await
                .unwrap();
                outbox
                    .enqueue_with_id(
                        OutboxId::parse("crash-rec-claim").unwrap(),
                        make_request(b"claim-payload"),
                    )
                    .await
                    .unwrap();
                outbox
                    .attach_reverse_stream(Arc::new(TcpReverseSink { stream_id: 1, addr }))
                    .await;
                let mut hold = String::new();
                let _ = std::io::stdin().read_line(&mut hold);
            }
            "write_before_ack_kill" => {
                let addr = std::env::var(ENV_RECEIVER_ADDR).expect("receiver addr");
                let outbox = CloudOutbox::open_default(&outbox_dir).await.unwrap();
                outbox
                    .enqueue_with_id(
                        OutboxId::parse("crash-rec-write").unwrap(),
                        make_request(b"write-payload"),
                    )
                    .await
                    .unwrap();
                outbox
                    .attach_reverse_stream(Arc::new(TcpReverseSink { stream_id: 1, addr }))
                    .await;
                let mut hold = String::new();
                let _ = std::io::stdin().read_line(&mut hold);
            }
            "receiver_reserve_then_wait_kill" => {
                let dedupe =
                    ReceiverDedupe::open_with_system_clock(&dedupe_dir, DedupeLimits::default())
                        .await
                        .unwrap();
                let key = DedupeKey {
                    account_id: "acct-crash".into(),
                    source_installation_id: "cloud-crash".into(),
                    target_installation_id: "home-crash".into(),
                    id: OutboxId::parse("rx-crash-1").unwrap(),
                };
                let decision = dedupe
                    .reserve(key, "digest-rx-1".into())
                    .await
                    .expect("reserve");
                assert_eq!(decision, DedupeDecision::Execute);
                println!("READY_RESERVED");
                let _ = std::io::stdout().flush();
                let mut hold = String::new();
                let _ = std::io::stdin().read_line(&mut hold);
            }
            other => panic!("unknown crash scenario: {other}"),
        }
    });
}

#[tokio::test]
async fn crash_between_enqueue_and_write_delivers_exactly_once_after_restart() {
    let temp = TempDirGuard::new("domios-crash-enqueue");
    let outbox_dir = temp.path().join("outbox");
    let dedupe_dir = temp.path().join("dedupe");

    let mut child = spawn_crash_child("enqueue_then_wait_kill", &outbox_dir, &dedupe_dir, None);
    wait_for_child_marker(&mut child, "READY_ENQUEUED");
    let mut guard = KillOnDrop(Some(child));
    guard.kill_and_wait();

    let (addr, counts, _, _) = start_mock_receiver(dedupe_dir, None);
    let observer = SettledObserver::new();
    let reopened = CloudOutbox::open(
        &outbox_dir,
        CloudOutboxConfig {
            observer: observer.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    reopened
        .attach_reverse_stream(Arc::new(TcpReverseSink { stream_id: 2, addr }))
        .await;

    let id = OutboxId::parse("crash-rec-enqueue").unwrap();
    let status = observer.wait(&id).await;
    assert!(matches!(
        status,
        OutboxRecordStatus::Delivered {
            bytes_written: Some(true),
            ..
        }
    ));
    assert_eq!(counts.lock().get("crash-rec-enqueue").copied(), Some(1));
    assert_eq!(reopened.snapshot().queued_count, 0);
}

#[tokio::test]
async fn crash_after_durable_claim_before_wire_write_becomes_delivery_unconfirmed_never_replayed() {
    let temp = TempDirGuard::new("domios-crash-claim");
    let outbox_dir = temp.path().join("outbox");
    let dedupe_dir = temp.path().join("dedupe");
    let (addr, counts, _, _) = start_mock_receiver(dedupe_dir, None);

    let mut child = spawn_crash_child(
        "claim_before_wire_kill",
        &outbox_dir,
        &temp.path().join("unused"),
        Some(&addr),
    );
    wait_for_child_marker(&mut child, "READY_CLAIMED");
    let mut guard = KillOnDrop(Some(child));
    guard.kill_and_wait();

    let observer = SettledObserver::new();
    let reopened = CloudOutbox::open(
        &outbox_dir,
        CloudOutboxConfig {
            observer: observer.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    let id = OutboxId::parse("crash-rec-claim").unwrap();
    let status = reopened.record_status(&id).expect("unconfirmed receipt");
    assert_eq!(status.code(), DELIVERY_UNCONFIRMED);
    assert_eq!(status.bytes_written(), None);
    assert!(matches!(
        status,
        OutboxRecordStatus::DeliveryUnconfirmed {
            reason: UnconfirmedReason::ProcessCrashAfterClaim,
            bytes_written: None,
            ..
        }
    ));

    // Reattaching stream must NOT replay the crashed claim, while new work delivers normally.
    reopened
        .attach_reverse_stream(Arc::new(TcpReverseSink { stream_id: 3, addr }))
        .await;
    let probe = reopened
        .enqueue_with_id(
            OutboxId::parse("probe-after-claim-crash").unwrap(),
            make_request(b"probe"),
        )
        .await
        .unwrap();
    let probe_status = observer.wait(&probe.id).await;
    assert!(matches!(probe_status, OutboxRecordStatus::Delivered { .. }));

    assert_eq!(counts.lock().get("crash-rec-claim").copied(), None);
    assert_eq!(
        counts.lock().get("probe-after-claim-crash").copied(),
        Some(1)
    );
}

#[tokio::test]
async fn crash_after_wire_write_before_ack_becomes_delivery_unconfirmed_and_receiver_executes_once()
{
    let temp = TempDirGuard::new("domios-crash-write");
    let outbox_dir = temp.path().join("outbox");
    let dedupe_dir = temp.path().join("dedupe");
    let (addr, counts, hold_hit, hold_flag) =
        start_mock_receiver(dedupe_dir, Some("crash-rec-write".into()));

    let wait_hit = hold_hit.notified();
    tokio::pin!(wait_hit);
    wait_hit.as_mut().enable();

    let child = spawn_crash_child(
        "write_before_ack_kill",
        &outbox_dir,
        &temp.path().join("unused"),
        Some(&addr),
    );
    let mut guard = KillOnDrop(Some(child));

    if hold_flag.load(Ordering::SeqCst) == 0 {
        tokio::time::timeout(Duration::from_secs(5), wait_hit)
            .await
            .expect("receiver observed frame from child");
    }
    // Receiver executed and completed the frame, and held ACK; now hard-kill sender!
    guard.kill_and_wait();

    let observer = SettledObserver::new();
    let reopened = CloudOutbox::open(
        &outbox_dir,
        CloudOutboxConfig {
            observer: observer.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    let id = OutboxId::parse("crash-rec-write").unwrap();
    let status = reopened.record_status(&id).expect("unconfirmed receipt");
    assert_eq!(status.code(), DELIVERY_UNCONFIRMED);
    assert!(matches!(
        status,
        OutboxRecordStatus::DeliveryUnconfirmed {
            reason: UnconfirmedReason::ProcessCrashAfterClaim,
            bytes_written: None,
            ..
        }
    ));

    // Attach new stream and send a follow-up message: crash-rec-write must NEVER replay.
    reopened
        .attach_reverse_stream(Arc::new(TcpReverseSink { stream_id: 4, addr }))
        .await;
    let followup = reopened
        .enqueue_with_id(
            OutboxId::parse("post-crash-followup").unwrap(),
            make_request(b"followup"),
        )
        .await
        .unwrap();
    let followup_status = observer.wait(&followup.id).await;
    assert!(matches!(
        followup_status,
        OutboxRecordStatus::Delivered { .. }
    ));

    assert_eq!(counts.lock().get("crash-rec-write").copied(), Some(1));
    assert_eq!(counts.lock().get("post-crash-followup").copied(), Some(1));
}

#[tokio::test]
async fn receiver_crash_between_dedupe_reserve_and_complete_becomes_crash_uncertain() {
    let temp = TempDirGuard::new("domios-crash-rx");
    let outbox_dir = temp.path().join("outbox");
    let dedupe_dir = temp.path().join("dedupe");

    let mut child = spawn_crash_child(
        "receiver_reserve_then_wait_kill",
        &outbox_dir,
        &dedupe_dir,
        None,
    );
    wait_for_child_marker(&mut child, "READY_RESERVED");
    let mut guard = KillOnDrop(Some(child));
    guard.kill_and_wait();

    let reopened = ReceiverDedupe::open_with_system_clock(&dedupe_dir, DedupeLimits::default())
        .await
        .unwrap();
    let key = DedupeKey {
        account_id: "acct-crash".into(),
        source_installation_id: "cloud-crash".into(),
        target_installation_id: "home-crash".into(),
        id: OutboxId::parse("rx-crash-1").unwrap(),
    };
    let decision = reopened
        .reserve(key, "digest-rx-1".into())
        .await
        .expect("reserve after crash");
    assert_eq!(decision, DedupeDecision::CrashUncertain);
}
