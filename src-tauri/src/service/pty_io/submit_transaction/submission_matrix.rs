//! WHAT: provider-capability submission coverage through shared production seams.
//! WHY: descriptor rows must expand coverage without copied assertions or quiet holes.
//! ORACLE: every dimension prints PASS or a quota-named SKIP and records exact writes.
use super::*;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::pty::screen_state::ScreenState;
use crate::pty::write_queue::{GuardedInputGate, WriteQueue};
use crate::service::delivery_error::DeliveryError;
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::sync::{Arc, Barrier, Mutex as StdMutex};
use std::time::Duration;

const QUOTA_SKIP: &str =
    "quota constraint: Cursor scope is startup + exact ingestion; submit writes are forbidden";

#[derive(Deserialize)]
struct ProviderDescriptor {
    id: String,
    spawn: SpawnCapabilities,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnCapabilities {
    readiness_strategy: Strategy,
    verify_submit: Strategy,
    has_hooks: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Strategy {
    kind: String,
    timeout_secs: Option<u64>,
}

#[derive(Default)]
struct RecordingTransport {
    generation: Cell<u64>,
    turn_started: Cell<bool>,
    fast_completion: Cell<bool>,
    swallowed_submit_keys: Cell<usize>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    turns: Cell<usize>,
    draft: RefCell<Vec<u8>>,
    payloads: RefCell<Vec<Vec<u8>>>,
    events: RefCell<Vec<&'static str>>,
}

impl RecordingTransport {
    fn fast() -> Self {
        let transport = Self::default();
        transport.fast_completion.set(true);
        transport
    }

    fn swallowing(count: usize) -> Self {
        let transport = Self::default();
        transport.swallowed_submit_keys.set(count);
        transport
    }
}

impl SubmitTransport for RecordingTransport {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: String::from_utf8_lossy(&self.draft.borrow()).into_owned(),
        })
    }

    fn draft_evidence_count(&self, text: &str) -> usize {
        usize::from(self.draft.borrow().as_slice() == text.as_bytes())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        self.turn_started.get()
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.payloads.borrow_mut().push(text.as_bytes().to_vec());
        self.draft.replace(text.as_bytes().to_vec());
        self.events.borrow_mut().extend(["payload", "ingested"]);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        self.events.borrow_mut().push("submit");
        if self.swallowed_submit_keys.get() > 0 {
            self.swallowed_submit_keys
                .set(self.swallowed_submit_keys.get() - 1);
            return Ok(());
        }
        if self.draft.borrow().is_empty() {
            return Ok(());
        }
        self.draft.borrow_mut().clear();
        self.turns.set(self.turns.get() + 1);
        self.turn_started.set(!self.fast_completion.get());
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

fn opts() -> VerifyOpts {
    VerifyOpts {
        require_prompt_first: false,
        verify_echo: true,
        verify_submit: true,
        prompt_timeout: Duration::ZERO,
        echo_timeout: Duration::ZERO,
        // Non-expired budget keeps the bounded key-only recovery reachable;
        // the mock transports never block a wait, so no real time is spent.
        submit_timeout: Duration::from_secs(5),
    }
}

fn strategy(capabilities: &SpawnCapabilities) -> VerifySubmitStrategy {
    assert_eq!(capabilities.verify_submit.kind, "tui_input_drained");
    VerifySubmitStrategy::TuiInputDrained
}

fn pass(provider: &str, dimension: &str) {
    writeln!(std::io::stderr().lock(), "PASS {provider} {dimension}").unwrap();
}

fn skip(provider: &str, dimension: &str) {
    writeln!(
        std::io::stderr().lock(),
        "SKIP {provider} {dimension}: {QUOTA_SKIP}"
    )
    .unwrap();
}

fn readiness(provider: &str, capabilities: &SpawnCapabilities) {
    assert_eq!(capabilities.readiness_strategy.kind, "poll_shell_prompt");
    let timeout = capabilities
        .readiness_strategy
        .timeout_secs
        .expect("observable readiness must carry a bounded deadline");
    assert!((1..=30).contains(&timeout));
    let mut screen = ScreenState::new();
    assert!(screen.bind_pty_generation(provider, 1));
    let signal = screen.arm_shell_ready_signal(provider);
    assert!(!signal.is_ready());
    assert!(screen.process_if_generation(provider, 1, b"\x1b]133;A\x07"));
    assert!(signal.is_ready());
    pass(provider, "readiness");
}

fn exact_payload(
    provider: &str,
    capabilities: &SpawnCapabilities,
    dimension: &str,
    payload: &str,
    submit: bool,
) -> usize {
    let transport = RecordingTransport::default();
    let result = run(&transport, payload, submit, strategy(capabilities), opts()).unwrap();
    assert_eq!(
        transport.payloads.borrow().as_slice(),
        &[payload.as_bytes().to_vec()]
    );
    assert_eq!(transport.payload_writes.get(), 1);
    assert_eq!(
        transport.submit_key_writes.get(),
        if submit { MAX_SUBMIT_KEY_WRITES } else { 0 }
    );
    assert_eq!(transport.turns.get(), usize::from(submit));
    assert_eq!(result.submit_observed, submit);
    pass(provider, dimension);
    transport.submit_key_writes.get()
}

fn enter(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::default();
    let result = run(
        &transport,
        "enter-contract",
        true,
        strategy(capabilities),
        opts(),
    )
    .unwrap();
    assert!(result.submit_observed);
    assert_eq!(transport.turns.get(), 1);
    pass(provider, "enter");
}

fn shift_enter(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::default();
    let payload = "first line\nsecond line";
    let result = run(&transport, payload, false, strategy(capabilities), opts()).unwrap();
    assert_eq!(transport.draft.borrow().as_slice(), payload.as_bytes());
    assert_eq!(transport.submit_key_writes.get(), 0);
    assert_eq!(transport.turns.get(), 0);
    assert!(!result.submit_observed);
    pass(provider, "shift_enter_newline_without_submit");
}

fn ime(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::default();
    let composing = "にほん";
    run(&transport, composing, false, strategy(capabilities), opts()).unwrap();
    assert_eq!(transport.submit_key_writes.get(), 0);
    assert_eq!(transport.turns.get(), 0);
    pass(provider, "ime_no_mid_composition_submit");
}

fn paste_before_submit(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::default();
    run(
        &transport,
        "causal-order",
        true,
        strategy(capabilities),
        opts(),
    )
    .unwrap();
    let events = transport.events.borrow();
    let first_submit = events
        .iter()
        .position(|event| *event == "submit")
        .expect("the bounded key-only burst must submit");
    assert_eq!(&events[..first_submit], &["payload", "ingested"]);
    assert!(events[first_submit..]
        .iter()
        .all(|event| *event == "submit"));
    pass(provider, "paste_ingested_before_submit");
}

fn evidence_tier(provider: &str, capabilities: &SpawnCapabilities) {
    assert_eq!(capabilities.verify_submit.kind, "tui_input_drained");
    let mut record =
        PendingDeliveryRecord::new("tier".into(), PendingDeliveryChannel::InteractiveInput);
    record.mark_ingested();
    crate::service::launch_delivery::confirm_fallback(provider, &mut record, true).unwrap();
    assert_eq!(record.state(), PendingDeliveryState::AcceptedWeak);
    pass(provider, "evidence_tier_accepted_weak");
}

fn optimistic_reconciliation(provider: &str) {
    let mut record = PendingDeliveryRecord::new(
        "optimistic".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    assert_eq!(record.state(), PendingDeliveryState::Pending);
    crate::service::launch_delivery::confirm_fallback(provider, &mut record, true).unwrap();
    assert!(record.state().is_accepted());
    pass(provider, "optimistic_reconciliation_terminal");
}

fn persistence(provider: &str) {
    let mut record = PendingDeliveryRecord::new(
        "persistent".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    record.confirm().unwrap();
    let serialized = serde_json::to_string(&record).unwrap();
    let restored: PendingDeliveryRecord = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.state(), PendingDeliveryState::AcceptedWeak);
    assert_eq!(restored.content_digest, "persistent");
    pass(provider, "persistence_across_remount");
}

fn fast_completion(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::fast();
    let result = run(
        &transport,
        "fast-completion",
        true,
        strategy(capabilities),
        opts(),
    )
    .unwrap();
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
    assert!(!transport.turn_started());
    pass(provider, "fast_completion_requires_turn_evidence");
}

fn swallowed_enter(provider: &str, capabilities: &SpawnCapabilities) {
    let transport = RecordingTransport::swallowing(1);
    let result = run(
        &transport,
        "single-payload",
        true,
        strategy(capabilities),
        opts(),
    )
    .unwrap();
    assert!(result.submit_observed);
    assert_eq!(transport.payload_writes.get(), 1);
    assert!(transport.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
    assert_eq!(transport.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
    pass(provider, "bounded_key_only_swallowed_enter_retry");
}

fn visible_failure_and_retry(provider: &str) {
    let error = DeliveryError::SubmitUnconfirmed {
        tab: provider.into(),
        detail: "submit key was not acknowledged".into(),
    };
    let json = error.to_json();
    assert_eq!(json["error"]["code"], "submit_unconfirmed");
    assert_eq!(json["error"]["bytes_written"], true);
    let mut record =
        PendingDeliveryRecord::new("retry".into(), PendingDeliveryChannel::InteractiveInput);
    record.mark_unconfirmed();
    record
        .begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "retry-1".into(),
            started_at_ms: 1,
        })
        .unwrap();
    assert_eq!(record.state(), PendingDeliveryState::Unconfirmed);
    assert_eq!(
        record
            .attempt_provenance()
            .map(|attempt| attempt.attempt_id.as_str()),
        Some("retry-1")
    );
    pass(provider, "visible_failure_and_retry");
}

fn busy_provider(provider: &str) {
    let error = DeliveryError::from_injection(
        provider,
        InjectionError::AgentTuiPresent {
            pty_id: provider.into(),
        },
    );
    assert_eq!(error.code(), "tab_busy_refused");
    assert!(!error.bytes_written());
    pass(provider, "busy_provider_zero_write");
}

struct BufferWriter(Arc<StdMutex<Vec<u8>>>);

impl Write for BufferWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn isolation(provider: &str) {
    let first = Arc::new(StdMutex::new(Vec::new()));
    let second = Arc::new(StdMutex::new(Vec::new()));
    let first_queue = Arc::new(WriteQueue::new(Box::new(BufferWriter(first.clone()))));
    let second_queue = Arc::new(WriteQueue::new(Box::new(BufferWriter(second.clone()))));
    let first_gate = Arc::new(GuardedInputGate::default());
    let second_gate = Arc::new(GuardedInputGate::default());
    first_gate.rearm();
    second_gate.rearm();
    let barrier = Arc::new(Barrier::new(3));
    let handles = [
        (
            first_queue.clone(),
            first_gate.issue().unwrap(),
            barrier.clone(),
            b"tab-a".to_vec(),
        ),
        (
            second_queue.clone(),
            second_gate.issue().unwrap(),
            barrier.clone(),
            b"tab-b".to_vec(),
        ),
    ]
    .into_iter()
    .map(|(queue, permit, barrier, payload)| {
        std::thread::spawn(move || {
            barrier.wait();
            queue
                .send_and_wait_guarded(payload, permit, Duration::from_secs(1))
                .unwrap();
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(first.lock().unwrap().as_slice(), b"tab-a");
    assert_eq!(second.lock().unwrap().as_slice(), b"tab-b");

    let stale = first_gate.issue().unwrap();
    first_gate.invalidate();
    first_gate.rearm();
    assert!(first_queue
        .send_and_wait_guarded(b"stale".to_vec(), stale, Duration::from_secs(1))
        .is_err());
    first_queue
        .send_and_wait_guarded(
            b"-fresh".to_vec(),
            first_gate.issue().unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(first.lock().unwrap().as_slice(), b"tab-a-fresh");
    assert_eq!(second.lock().unwrap().as_slice(), b"tab-b");
    pass(provider, "close_rebind_isolation");
}

fn run_full_matrix(descriptor: &ProviderDescriptor) {
    let provider = descriptor.id.as_str();
    enter(provider, &descriptor.spawn);
    shift_enter(provider, &descriptor.spawn);
    ime(provider, &descriptor.spawn);
    paste_before_submit(provider, &descriptor.spawn);
    evidence_tier(provider, &descriptor.spawn);
    optimistic_reconciliation(provider);
    persistence(provider);
    fast_completion(provider, &descriptor.spawn);
    swallowed_enter(provider, &descriptor.spawn);
    visible_failure_and_retry(provider);
    busy_provider(provider);
    isolation(provider);
}

fn run_provider(manifest: &str) {
    let descriptor: ProviderDescriptor = serde_json::from_str(manifest).unwrap();
    let provider = descriptor.id.as_str();
    readiness(provider, &descriptor.spawn);
    let mut submit_writes = 0;
    submit_writes += exact_payload(
        provider,
        &descriptor.spawn,
        "ascii_payload_integrity",
        "ASCII payload 0123456789",
        descriptor.spawn.has_hooks,
    );
    submit_writes += exact_payload(
        provider,
        &descriptor.spawn,
        "unicode_payload_integrity",
        "Zażółć 🧪 e\u{301} 日本語",
        descriptor.spawn.has_hooks,
    );
    let multiline = "line\n".repeat(1000);
    assert_eq!(multiline.chars().count(), 5000);
    submit_writes += exact_payload(
        provider,
        &descriptor.spawn,
        "5k_multiline_payload_integrity",
        &multiline,
        descriptor.spawn.has_hooks,
    );

    if descriptor.spawn.has_hooks {
        run_full_matrix(&descriptor);
    } else {
        for dimension in [
            "enter",
            "shift_enter_newline_without_submit",
            "ime_no_mid_composition_submit",
            "paste_ingested_before_submit",
            "evidence_tier",
            "optimistic_reconciliation_terminal",
            "persistence_across_remount",
            "fast_completion_requires_turn_evidence",
            "bounded_key_only_swallowed_enter_retry",
            "visible_failure_and_retry",
            "busy_provider_zero_write",
            "close_rebind_isolation",
        ] {
            skip(provider, dimension);
        }
        assert_eq!(
            submit_writes, 0,
            "restricted providers write zero submit keys"
        );
    }
}

macro_rules! provider_matrix {
    ($($provider:ident => $manifest:literal),+ $(,)?) => {
        $(
            mod $provider {
                #[test]
                fn provider_capability_matrix() {
                    super::run_provider(include_str!($manifest));
                }
            }
        )+
    };
}

provider_matrix! {
    claude => "../../../../../plugins/provider-claude/plugin.json",
    codex => "../../../../../plugins/provider-codex/plugin.json",
    opencode => "../../../../../plugins/provider-opencode/plugin.json",
    cursor => "../../../../../plugins/provider-cursor/plugin.json",
}
