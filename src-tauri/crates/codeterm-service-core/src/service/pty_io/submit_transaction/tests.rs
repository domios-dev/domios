//! WHAT: deterministic submit-transaction experiments at the transport seam.
//! WHY: delivery confidence must survive adversarial ingestion and acknowledgement traces.
//! ORACLE: recorded writes and classifications expose every false submit claim.
use super::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;

#[test]
fn refused_ingress_claim_preserves_the_written_submit_cycle() {
    let first = claim_submit_cycle("rapid-ingress-claim");
    let (sender, _) = std::sync::mpsc::sync_channel(1);
    first.payload_enqueued(sender);
    let second = claim_submit_cycle("rapid-ingress-claim");

    assert!(first.is_current());
    assert!(second.is_current());
    drop(second);
    assert!(first.is_current());
}

struct ControlledTransport {
    tail: &'static str,
    generation: Cell<u64>,
    turn: Cell<bool>,
    scheduled_waits: Cell<usize>,
    writes: RefCell<Vec<String>>,
}

impl ControlledTransport {
    fn new(tail: &'static str) -> Self {
        Self {
            tail,
            generation: Cell::new(0),
            turn: Cell::new(false),
            scheduled_waits: Cell::new(0),
            writes: RefCell::new(Vec::new()),
        }
    }
}

impl SubmitTransport for ControlledTransport {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: self.tail.to_string(),
        })
    }

    // Ordering model: the paste render frame is delivered synchronously by
    // `write_paste`, so ingestion always resolves and Enter follows the paste.
    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.generation.get() > 0)
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        self.turn.get()
    }

    fn wait_retry_deadline(&self, _deadline: Instant) {
        self.scheduled_waits.set(self.scheduled_waits.get() + 1);
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.generation.set(self.generation.get() + 1);
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(4)
    }

    fn write_submit_key(&self, key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        let name = if key == b"\t" { "tab" } else { "enter" };
        self.writes
            .borrow_mut()
            .push(format!("{name}:{}", self.tail));
        self.turn.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

#[test]
fn first_submit_key_waits_for_declared_ingestion() {
    let io = ControlledTransport::new("> prompt");
    let mut options = opts();
    options.verify_submit = false;
    let result = run(
        &io,
        "1",
        true,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::from_millis(150),
            retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        options,
    )
    .unwrap();

    assert_eq!(io.scheduled_waits.get(), 1);
    assert_eq!(&*io.writes.borrow(), &["paste:1", "enter:> prompt"]);
    assert_eq!(result.submit_key_writes, 1);
}

struct ComposerRedrawWithoutTurn {
    generation: Cell<u64>,
    draft_visible: Cell<bool>,
    writes: RefCell<Vec<String>>,
}

impl ComposerRedrawWithoutTurn {
    fn new() -> Self {
        Self {
            generation: Cell::new(0),
            draft_visible: Cell::new(false),
            writes: RefCell::new(Vec::new()),
        }
    }
}

struct ClearedComposerWithoutObservedTurn {
    generation: Cell<u64>,
    draft_visible: Cell<bool>,
}

impl ClearedComposerWithoutObservedTurn {
    fn new() -> Self {
        Self {
            generation: Cell::new(0),
            draft_visible: Cell::new(false),
        }
    }
}

impl SubmitTransport for ClearedComposerWithoutObservedTurn {
    fn observe(&self) -> Option<Observation> {
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.draft_visible.set(false);
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

impl SubmitTransport for ComposerRedrawWithoutTurn {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: "> [Pasted text #1 +45 lines]".into(),
        })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.draft_visible.set(false);
        self.generation.set(self.generation.get() + 1);
        self.writes.borrow_mut().push("enter".into());
        Ok(())
    }
}

pub(super) fn opts() -> VerifyOpts {
    VerifyOpts {
        require_prompt_first: false,
        verify_echo: true,
        verify_submit: true,
        echo_timeout: Duration::ZERO,
        // Synchronous mock responses keep bounded retries reachable;
        // an expired deadline forbids retry writes entirely.
        submit_timeout: Duration::from_secs(5),
        prompt_timeout: Duration::ZERO,
    }
}

pub(super) fn tui() -> VerifySubmitStrategy {
    VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::ZERO,
        retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
    }
}

struct HalfIngestion {
    generation: Cell<u64>,
    evidence: Cell<usize>,
    advance_generation: bool,
    raise_evidence: bool,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
}

impl HalfIngestion {
    fn evidence_without_generation() -> Self {
        Self::new(false, true)
    }

    fn generation_without_evidence() -> Self {
        Self::new(true, false)
    }

    fn new(advance_generation: bool, raise_evidence: bool) -> Self {
        Self {
            generation: Cell::new(0),
            evidence: Cell::new(0),
            advance_generation,
            raise_evidence,
            payload_writes: Cell::new(0),
            submit_key_writes: Cell::new(0),
        }
    }
}

impl SubmitTransport for HalfIngestion {
    fn observe(&self) -> Option<Observation> {
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        self.evidence.get()
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        if self.advance_generation {
            self.generation.set(self.generation.get() + 1);
        }
        if self.raise_evidence {
            self.evidence.set(self.evidence.get() + 1);
        }
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        Ok(())
    }
}

mod ingestion_gate {
    use super::*;

    fn assert_first_submit_key_is_unconditional(io: &HalfIngestion) {
        let result = run(io, "causal-ingestion", true, tui(), opts()).unwrap();

        assert!(result.submit_anomaly);
        assert_eq!(io.payload_writes.get(), 1);
        assert!(io.submit_key_writes.get() >= 1);
    }

    #[test]
    fn evidence_without_generation_still_reaches_submit_key() {
        assert_first_submit_key_is_unconditional(&HalfIngestion::evidence_without_generation());
    }

    #[test]
    fn generation_without_evidence_still_reaches_submit_key() {
        assert_first_submit_key_is_unconditional(&HalfIngestion::generation_without_evidence());
    }
}

mod fast_turn {
    use super::*;

    struct SettlesBeforeFirstWait {
        generation: Cell<u64>,
        draft_visible: Cell<bool>,
        wait_calls: Cell<usize>,
        submit_key_writes: Cell<usize>,
    }

    impl SettlesBeforeFirstWait {
        fn new() -> Self {
            Self {
                generation: Cell::new(0),
                draft_visible: Cell::new(false),
                wait_calls: Cell::new(0),
                submit_key_writes: Cell::new(0),
            }
        }
    }

    impl SubmitTransport for SettlesBeforeFirstWait {
        fn observe(&self) -> Option<Observation> {
            Some(Observation { tail: "> ".into() })
        }

        fn draft_evidence_count(&self, _text: &str) -> usize {
            usize::from(self.draft_visible.get())
        }

        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
            self.wait_calls.set(self.wait_calls.get() + 1);
            if self.wait_calls.get() != 1 {
                return false;
            }
            self.draft_visible.set(false);
            self.generation.set(self.generation.get() + 1);
            true
        }

        fn turn_started(&self) -> bool {
            false
        }

        fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
            self.draft_visible.set(true);
            self.generation.set(self.generation.get() + 1);
            Ok(text.len())
        }

        fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
            self.submit_key_writes.set(self.submit_key_writes.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn settled_screen_without_observed_turn_stays_unconfirmed() {
        let io = SettlesBeforeFirstWait::new();
        let result = run(&io, "fast-turn", true, tui(), opts()).unwrap();

        assert_eq!(io.wait_calls.get(), 0);
        assert_eq!(io.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
        assert!(!result.submit_observed);
        assert!(result.submit_anomaly);
    }
}

struct IndistinguishableTrace {
    prompt_accepted: bool,
    generation: Cell<u64>,
    draft_visible: Cell<bool>,
    payload_writes: Cell<usize>,
    payload: RefCell<Option<String>>,
    submit_key_writes: Cell<usize>,
    observations: RefCell<Vec<String>>,
}

impl IndistinguishableTrace {
    fn fast_completed() -> Self {
        Self::new(true)
    }

    fn swallowed() -> Self {
        Self::new(false)
    }

    fn new(prompt_accepted: bool) -> Self {
        Self {
            prompt_accepted,
            generation: Cell::new(0),
            draft_visible: Cell::new(false),
            payload_writes: Cell::new(0),
            payload: RefCell::new(None),
            submit_key_writes: Cell::new(0),
            observations: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, observation: String) {
        self.observations.borrow_mut().push(observation);
    }
}

impl SubmitTransport for IndistinguishableTrace {
    fn observe(&self) -> Option<Observation> {
        self.record("observe:composer".into());
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        let evidence = usize::from(self.draft_visible.get());
        self.record(format!("draft:{evidence}"));
        evidence
    }

    fn generation(&self) -> u64 {
        let generation = self.generation.get();
        self.record(format!("generation:{generation}"));
        generation
    }

    fn wait_change(&self, generation: u64, _deadline: Instant) -> bool {
        self.record(format!("wait_after:{generation}:false"));
        false
    }

    fn turn_started(&self) -> bool {
        self.record("turn_started:false".into());
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        *self.payload.borrow_mut() = Some(text.to_string());
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        self.record("write_paste".into());
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        self.draft_visible.set(false);
        self.generation.set(self.generation.get() + 1);
        self.record("write_submit_key".into());
        Ok(())
    }
}

mod swallowed_vs_fast {
    use super::*;

    #[test]
    fn accepted_fast_turn_and_swallowed_enter_are_observationally_identical() {
        let fast = IndistinguishableTrace::fast_completed();
        let swallowed = IndistinguishableTrace::swallowed();

        let fast_result = run(&fast, "same-prompt", true, tui(), opts()).unwrap();
        let swallowed_result = run(&swallowed, "same-prompt", true, tui(), opts()).unwrap();

        assert!(fast.prompt_accepted);
        assert!(!swallowed.prompt_accepted);
        assert_eq!(
            &*fast.observations.borrow(),
            &*swallowed.observations.borrow()
        );
        assert_eq!(
            fast_result.submit_observed,
            swallowed_result.submit_observed
        );
        assert_eq!(fast_result.submit_anomaly, swallowed_result.submit_anomaly);
        assert!(!fast_result.submit_observed);
        assert!(fast_result.submit_anomaly);
    }
}

mod bounded_recovery {
    use super::*;

    #[test]
    fn total_acknowledgement_failure_is_payload_singular_and_key_bounded() {
        let io = IndistinguishableTrace::swallowed();

        let result = run(&io, "singular-payload", true, tui(), opts()).unwrap();

        assert_eq!(io.payload_writes.get(), 1);
        assert!(io.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
        assert_eq!(io.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
        assert!(!result.submit_observed);
        assert!(result.submit_anomaly);
    }
}

#[test]
fn cleared_tui_composer_without_observed_turn_stays_unconfirmed() {
    let result = run(
        &ClearedComposerWithoutObservedTurn::new(),
        "fast opencode turn",
        true,
        tui(),
        opts(),
    )
    .unwrap();

    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
}

#[test]
fn composer_region_excludes_matching_history_after_submission() {
    let lines = vec![
        "› routed payload".to_string(),
        "• processed routed payload".to_string(),
        "› Find and fix a bug in @filename".to_string(),
        "gpt-5.6-sol".to_string(),
    ];

    let region = composer_region_text(&lines, 2);

    assert!(!region.contains("routed payload"));
    assert!(region.contains("Find and fix a bug"));
}

#[test]
fn composer_region_keeps_wrapped_draft_through_cursor_row() {
    let lines = vec![
        "• prior output".to_string(),
        "› first draft line".to_string(),
        "  second draft line".to_string(),
        "  third draft line".to_string(),
        "gpt-5.6-sol".to_string(),
    ];

    let region = composer_region_text(&lines, 3);

    assert!(region.contains("first draft line"));
    assert!(region.contains("third draft line"));
    assert!(!region.contains("prior output"));
}

#[test]
fn composer_region_removes_visual_padding_at_a_mid_token_wrap() {
    let expected = "убери тут paused пожалуйста из списка в команде там C:\\Users\\dev\\AppData\\Local\\Temp\\codeterm-chat\\237d67c5-pasted-image-1785493045962.png";
    let lines = vec![
        "────────────────────────────────────────────────────────".to_string(),
        "›  убери тут paused пожалуйста из списка в команде там C:\\Users\\dev\\AppData\\Local\\Temp\\codeterm-chat\\237d67c5-".to_string(),
        "  pasted-image-1785493045962.png".to_string(),
        "gpt-5.6-sol".to_string(),
    ];

    let region = composer_region_text(&lines, 2);

    assert_eq!(screen_echo_evidence_count(&region, expected), 1);
}

#[test]
fn tui_composer_redraw_without_turn_is_not_submission() {
    let io = ComposerRedrawWithoutTurn::new();
    let result = run(&io, "general bootstrap", true, tui(), opts()).unwrap();
    let result = result.require_submission(true, Duration::ZERO, "> general bootstrap".into());

    assert!(
        matches!(&result, Err(InjectionError::SubmitNeverConfirmed { .. })),
        "{result:?}"
    );
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1
    );
}

#[test]
fn routed_prefix_body_and_suffix_share_one_write_before_the_submit_key() {
    let trace = IndistinguishableTrace::swallowed();
    let prefix = "<domios from=\"tab\" label=\"sender\" tab=\"abc12345\">";
    let body = "opaque routed body";
    let suffix = " [return hint] instruction</domios>";

    run_with_submit_key_and_payload(
        &trace,
        &format!("{prefix}{body}{suffix}"),
        true,
        b"\r",
        &format!("{prefix}{body}{suffix}"),
        VerifySubmitStrategy::None,
        opts(),
        |_, _| Ok(()),
    )
    .unwrap();

    assert_eq!(trace.payload_writes.get(), 1);
    let expected_write = format!("{prefix}{body}{suffix}");
    assert_eq!(
        trace.payload.borrow().as_deref(),
        Some(expected_write.as_str())
    );
    assert_eq!(trace.submit_key_writes.get(), 1);
    let writes = trace.observations.borrow();
    let payload = writes
        .iter()
        .position(|write| write == "write_paste")
        .unwrap();
    let key = writes
        .iter()
        .position(|write| write == "write_submit_key")
        .unwrap();
    assert!(payload < key);
}

#[test]
fn pasted_follow_up_shares_the_single_payload_write_before_the_submit_key() {
    let trace = IndistinguishableTrace::swallowed();
    let body = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta";
    let follow_up = "\n::marker::";

    run_with_submit_key_and_follow_up(
        &trace,
        body,
        true,
        b"\r",
        Some(follow_up),
        VerifySubmitStrategy::None,
        opts(),
        |_, _| Ok(()),
    )
    .unwrap();

    assert_eq!(trace.payload_writes.get(), 1);
    let expected_write = format!("{body}{follow_up}");
    assert_eq!(
        trace.payload.borrow().as_deref(),
        Some(expected_write.as_str())
    );
    assert_eq!(trace.submit_key_writes.get(), 1);
}

#[test]
fn tui_turn_transition_confirms_submission() {
    let io = ControlledTransport::new("> [Pasted text #1 +45 lines]");
    let result = run(&io, "general bootstrap", true, tui(), opts()).unwrap();

    assert!(result.submit_observed);
    assert!(io.turn_started());
    let writes = io.writes.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.starts_with("enter:"))
            .count(),
        1,
        "exact turn evidence stops the remaining key-only schedule"
    );
}
