//! WHAT: deterministic submit-transaction experiments at the transport seam.
//! WHY: delivery confidence must survive adversarial ingestion and acknowledgement traces.
//! ORACLE: recorded writes and classifications expose every false submit claim.
use super::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;

#[test]
fn xterm_paste_normalizes_newlines_and_obeys_the_live_mode() {
    let text = "alpha\r\nbeta\ngamma\rdelta";
    assert_eq!(
        xterm_paste(text, Some(true), PasteEncodingCapability::LiveModeOnly),
        "\x1b[200~alpha\rbeta\rgamma\rdelta\x1b[201~"
    );
    assert_eq!(
        xterm_paste(text, Some(false), PasteEncodingCapability::MultilineTui),
        "alpha\rbeta\rgamma\rdelta"
    );
    for strategy in [
        VerifySubmitStrategy::ShellPrompt {
            timeout: Duration::ZERO,
        },
        VerifySubmitStrategy::None,
    ] {
        assert_eq!(
            xterm_paste(text, None, PasteEncodingCapability::for_strategy(&strategy)),
            "alpha\rbeta\rgamma\rdelta"
        );
    }
}

#[test]
fn xterm_paste_keeps_single_line_chat_input_in_one_paste_frame() {
    assert_eq!(
        xterm_paste("payload", Some(true), PasteEncodingCapability::LiveModeOnly),
        "\x1b[200~payload\x1b[201~"
    );
}

#[derive(Default)]
struct SubmitSemantics {
    payload_writes: usize,
    bracketed_insertions: usize,
    submit_boundaries: usize,
    provider_rows: Vec<String>,
}

fn model_submit_semantics(frames: &[Vec<u8>]) -> SubmitSemantics {
    let mut semantics = SubmitSemantics::default();
    let mut composer = String::new();
    for frame in frames {
        let frame = std::str::from_utf8(frame).expect("oracle frames are UTF-8");
        if frame != "\r" {
            semantics.payload_writes += 1;
        }
        if let Some(content) = frame
            .strip_prefix(BRACKETED_PASTE_START)
            .and_then(|content| content.strip_suffix(BRACKETED_PASTE_END))
        {
            semantics.bracketed_insertions += 1;
            composer.push_str(content);
            continue;
        }
        for ch in frame.chars() {
            if ch == '\r' {
                semantics.submit_boundaries += 1;
                semantics.provider_rows.push(std::mem::take(&mut composer));
            } else {
                composer.push(ch);
            }
        }
    }
    semantics
}

#[test]
fn multiline_tui_bootstrap_encodes_as_one_hidden_system_turn() {
    let opaque_body = "TOKEN_A7\nTOKEN_B9\nTOKEN_C3";
    let payload = crate::service::messenger::Messenger::from(
        crate::service::messenger::MessageEnvelope::codeterm_reminder("general-bootstrap"),
    )
    .wrap(opaque_body);
    assert_eq!(payload.matches(crate::message_markers::OPEN).count(), 1);

    let frames = initial_submit_frames(
        &payload,
        b"\r",
        None,
        PasteEncodingCapability::for_strategy(&tui()),
    );
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1], b"\r");

    let semantics = model_submit_semantics(&frames);
    assert_eq!(semantics.payload_writes, 1);
    assert_eq!(semantics.bracketed_insertions, 1);
    assert_eq!(semantics.submit_boundaries, 1);
    assert_eq!(semantics.provider_rows.len(), 1);

    let mut hidden_system = Vec::new();
    let mut ordinary_users = Vec::new();
    for row in &semantics.provider_rows {
        match crate::message_markers::parse_host_injected(row) {
            Some(marker) if marker.kind == "system" => hidden_system.push(marker),
            _ => ordinary_users.push(row),
        }
    }
    assert_eq!(hidden_system.len(), 1);
    assert!(ordinary_users.is_empty());
    let hidden_system = &hidden_system[0];
    assert_eq!(hidden_system.kind, "system");
    assert_eq!(hidden_system.attr("context"), Some("general-bootstrap"));
    assert_eq!(hidden_system.payload.replace('\r', "\n"), opaque_body);
}

#[test]
fn newer_ingress_claim_cancels_the_prior_submit_cycle() {
    let first = claim_submit_cycle("rapid-ingress-claim");
    let second = claim_submit_cycle("rapid-ingress-claim");

    assert!(!first.is_current());
    assert!(second.is_current());
}

// A CR before the post-paste frame reproduces bf1278870; the turn never starts.
// Only causal paste evidence may release the CR, and deadlines only fail.
struct TuiModel {
    generation: Cell<u64>,
    draft_pending: Cell<bool>,
    draft_visible: Cell<bool>,
    turn: Cell<bool>,
    stale_frames_before_ingest: Cell<usize>,
    deliver_ingest_frame: bool,
    accept_enter: bool,
    swallowed_enters: Cell<usize>,
    newline_enters: Cell<usize>,
    writes: RefCell<Vec<String>>,
}

impl TuiModel {
    /// Normal causal path: the paste render event is observable; the CR that
    /// follows it starts a turn.
    fn ingesting() -> Self {
        Self::new(0, true, true)
    }

    fn stale_frame_then_ingesting() -> Self {
        Self::new(1, true, true)
    }

    /// The paste may land without exposing a screen event.
    fn recovers_after_unobserved_ingest() -> Self {
        Self::new(0, false, true)
    }

    /// Anomaly that fails honestly: no render event and the CR is swallowed —
    /// the turn never starts, so success must not be claimed.
    fn never_ingests() -> Self {
        Self::new(0, false, false)
    }

    fn new(
        stale_frames_before_ingest: usize,
        deliver_ingest_frame: bool,
        accept_enter: bool,
    ) -> Self {
        Self {
            generation: Cell::new(0),
            draft_pending: Cell::new(false),
            draft_visible: Cell::new(false),
            turn: Cell::new(false),
            stale_frames_before_ingest: Cell::new(stale_frames_before_ingest),
            deliver_ingest_frame,
            accept_enter,
            swallowed_enters: Cell::new(0),
            newline_enters: Cell::new(0),
            writes: RefCell::new(Vec::new()),
        }
    }

    fn swallows_first_enter() -> Self {
        Self::swallows_enters(1)
    }

    fn swallows_enters(count: usize) -> Self {
        let model = Self::new(0, true, true);
        model.swallowed_enters.set(count);
        model
    }

    fn already_working_and_swallows_first_enter() -> Self {
        let model = Self::swallows_first_enter();
        model.turn.set(true);
        model
    }

    fn first_enter_inserts_newline() -> Self {
        let model = Self::new(0, true, true);
        model.newline_enters.set(1);
        model
    }
}

impl SubmitTransport for TuiModel {
    fn observe(&self) -> Option<Observation> {
        let tail = if self.turn.get() {
            "working".to_string()
        } else if self.draft_visible.get() {
            "> [Pasted Content 42 chars]".to_string()
        } else {
            "> ".to_string()
        };
        Some(Observation { tail })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        if self.stale_frames_before_ingest.get() > 0 {
            self.stale_frames_before_ingest
                .set(self.stale_frames_before_ingest.get() - 1);
            self.generation.set(self.generation.get() + 1);
            return true;
        }
        // Deliver the ingestion frame exactly once after the paste.
        if self.deliver_ingest_frame && self.draft_pending.get() && !self.draft_visible.get() {
            self.draft_visible.set(true);
            self.generation.set(self.generation.get() + 1);
            return true;
        }
        false
    }

    fn turn_started(&self) -> bool {
        self.turn.get()
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.draft_pending.set(true);
        if !self.deliver_ingest_frame {
            self.draft_visible.set(true);
        }
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(text.len())
    }

    fn write_submit_key(&self, key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        let name = if key == b"\t" { "tab" } else { "enter" };
        self.writes.borrow_mut().push(name.to_string());
        if self.deliver_ingest_frame && self.draft_pending.get() && !self.draft_visible.get() {
            self.draft_visible.set(true);
            self.generation.set(self.generation.get() + 1);
        }
        if self.swallowed_enters.get() > 0 {
            self.swallowed_enters.set(self.swallowed_enters.get() - 1);
            return Ok(());
        }
        if self.newline_enters.get() > 0 {
            self.newline_enters.set(self.newline_enters.get() - 1);
            self.generation.set(self.generation.get() + 1);
            return Ok(());
        }
        let paste_landed = self.draft_visible.get() || !self.deliver_ingest_frame;
        if self.accept_enter && paste_landed {
            self.turn.set(key == b"\r");
            self.draft_visible.set(false);
            self.generation.set(self.generation.get() + 1);
        }
        Ok(())
    }
}

// Enter always starts a turn so this model isolates paste-before-key ordering.
// `TuiModel` owns ingestion-race coverage.
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
fn provider_ingestion_offset_schedules_only_the_first_submit_key() {
    let io = ControlledTransport::new("> prompt");
    let mut options = opts();
    options.verify_submit = false;
    let result = run(
        &io,
        "1",
        true,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::from_millis(150),
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
        // Mock transports acknowledge or refuse synchronously (wait_change
        // never blocks); the budget only keeps bounded retries reachable —
        // an expired deadline forbids retry writes entirely.
        submit_timeout: Duration::from_secs(5),
        prompt_timeout: Duration::ZERO,
    }
}

pub(super) fn tui() -> VerifySubmitStrategy {
    VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::ZERO,
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
        MAX_SUBMIT_KEY_WRITES,
        "observed acceptance classifies but never stops the key-only burst"
    );
}

#[test]
fn first_submit_key_does_not_wait_for_paste_evidence() {
    let io = TuiModel::never_ingests();
    let result = run(&io, "general bootstrap", true, tui(), opts()).unwrap();

    assert!(result.submit_anomaly);
    assert!(!result.submit_observed);
    let writes = io.writes.borrow();
    assert_eq!(
        writes.first().map(String::as_str),
        Some("paste:general bootstrap")
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES,
        "a parked payload gets the full bounded key-only recovery budget"
    );
}

#[test]
fn zero_deadline_still_attempts_the_first_submit_key() {
    let io = TuiModel::never_ingests();
    let result = run(
        &io,
        "general bootstrap",
        true,
        tui(),
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            echo_timeout: Duration::ZERO,
            submit_timeout: Duration::ZERO,
            prompt_timeout: Duration::ZERO,
        },
    );

    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1
    );
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        1
    );
    let result = result.unwrap();
    assert!(result.submit_anomaly);
    assert!(!result.submit_observed);
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn normal_path_fires_cr_on_the_ingestion_event_with_no_anomaly() {
    // The ingestion frame causally releases Enter; a pipe flush alone is insufficient.
    // This fails against bf1278870, where paste+CR were written back-to-back.
    let io = TuiModel::ingesting();
    let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

    assert!(io.turn_started(), "the CR must start a real provider turn");
    assert!(
        result.submit_observed,
        "submit_observed must reflect the provider turn, not a ConPTY flush"
    );
    assert!(!result.submit_anomaly);
    assert_eq!(&*io.writes.borrow(), &["paste:unique-marker-42", "enter"]);
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn stale_repaint_cannot_release_enter_before_new_fuzzy_echo() {
    let io = TuiModel::stale_frame_then_ingesting();
    let result = run(&io, "pairing intro", true, tui(), opts()).unwrap();

    assert!(result.submit_observed);
    assert!(!result.submit_anomaly);
    assert_eq!(&*io.writes.borrow(), &["paste:pairing intro", "enter"]);
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn visible_paste_without_generation_still_submits() {
    let io = TuiModel::recovers_after_unobserved_ingest();
    let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

    assert!(!result.submit_anomaly);
    assert!(result.submit_observed);
    assert_eq!(&*io.writes.borrow(), &["paste:unique-marker-42", "enter"]);
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn missing_paste_evidence_surfaces_actionable_failure() {
    let io = TuiModel::never_ingests();
    let result = run(&io, "marker", true, tui(), opts()).unwrap();

    assert!(result.submit_anomaly);
    assert!(!result.submit_observed, "no turn => not confirmed");
    assert!(!io.turn_started());
    assert_eq!(
        &*io.writes.borrow(),
        &["paste:marker", "enter", "enter", "enter"],
        "the payload remains singular while the submit key is retried"
    );

    let surfaced = result.require_submission(true, Duration::ZERO, "> marker".into());
    assert!(matches!(
        surfaced,
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn paste_accepted_then_enter_swallowed_recovers_without_duplicate_payload() {
    let io = TuiModel::swallows_first_enter();
    let result = run(&io, "team bootstrap", true, tui(), opts()).unwrap();

    assert!(!result.submit_anomaly);
    assert!(result.submit_observed);
    assert!(io.turn_started());
    assert_eq!(
        &*io.writes.borrow(),
        &["paste:team bootstrap", "enter", "enter"]
    );
    result
        .require_submission(true, Duration::ZERO, "> team bootstrap".into())
        .unwrap();
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn enter_that_inserts_newline_retries_until_the_turn_starts() {
    let io = TuiModel::first_enter_inserts_newline();
    let result = run(&io, "multiline payload", true, tui(), opts()).unwrap();

    assert!(!result.submit_anomaly);
    assert!(result.submit_observed);
    assert!(io.turn_started());
    assert_eq!(
        &*io.writes.borrow(),
        &["paste:multiline payload", "enter", "enter"]
    );
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn repeated_swallowed_enters_use_bounded_submit_key_recovery() {
    let io = TuiModel::swallows_enters(5);
    let result = run(&io, "delivery-marker", true, tui(), opts()).unwrap();

    assert!(result.submit_anomaly);
    assert!(!result.submit_observed);
    assert!(!io.turn_started());
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1,
        "recovery must never paste the message twice"
    );
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES,
        "a pending transaction retries only the submit key within its fixed budget"
    );
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn working_provider_recovers_swallowed_enter_when_draft_leaves_composer() {
    let io = TuiModel::already_working_and_swallows_first_enter();
    let result = run(&io, "team bootstrap", true, tui(), opts()).unwrap();

    assert!(!result.submit_anomaly);
    assert!(result.submit_observed);
    assert_eq!(
        &*io.writes.borrow(),
        &["paste:team bootstrap", "enter", "enter"]
    );
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn every_pty_provider_strategy_preserves_paste_then_enter_order() {
    let strategies = [
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
        },
        VerifySubmitStrategy::ShellPrompt {
            timeout: Duration::ZERO,
        },
        VerifySubmitStrategy::None,
    ];

    for strategy in strategies {
        let io = ControlledTransport::new("> provider-neutral prompt");
        let result = run(&io, "provider-neutral prompt", true, strategy, opts()).unwrap();
        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[
                "paste:provider-neutral prompt",
                "enter:> provider-neutral prompt",
                "enter:> provider-neutral prompt",
                "enter:> provider-neutral prompt"
            ]
        );
    }
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn long_wrapped_chat_message_enters_after_paste_placeholder_echo() {
    let body = "continuation ".repeat(200);
    let io = ControlledTransport::new("> [Pasted Content 2600 chars]");
    let result = run(&io, &body, true, tui(), opts()).unwrap();

    assert!(result.submit_observed);
    assert!(result.echo_observed);
    assert_eq!(
        &*io.writes.borrow(),
        &[
            format!("paste:{body}"),
            "enter:> [Pasted Content 2600 chars]".to_string(),
            "enter:> [Pasted Content 2600 chars]".to_string(),
            "enter:> [Pasted Content 2600 chars]".to_string()
        ]
    );
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn complete_echo_is_diagnostic_but_not_required_for_enter() {
    let io = ControlledTransport::new("> hello");
    let result = run(&io, "hello", true, tui(), opts()).unwrap();
    assert!(result.echo_observed);
    assert!(result.submit_observed);
}

#[test]
#[ignore = "synthetic TUI model is not a real PTY delivery oracle"]
fn spawn_introduction_uses_the_same_paste_then_enter_contract() {
    let intro = "-=-codeterm:tab label=CodeTerm tab=system-=-You are the Orchestrator of this team. Your Planner counterpart is Rodin.";
    let io = ControlledTransport::new("> [Pasted Content 128 chars]");

    let result = submit_entry(PtySubmitEntry::Messenger, intro, |body| {
        run(&io, body, true, tui(), opts())
    })
    .unwrap();

    assert!(result.submit_observed);
    assert_eq!(
        &*io.writes.borrow(),
        &[
            format!("paste:{intro}"),
            "enter:> [Pasted Content 128 chars]".to_string(),
            "enter:> [Pasted Content 128 chars]".to_string(),
            "enter:> [Pasted Content 128 chars]".to_string()
        ]
    );
}

#[test]
fn messenger_report_planner_send_share_submit_transaction() {
    for entry in [
        PtySubmitEntry::Messenger,
        PtySubmitEntry::Report,
        PtySubmitEntry::Planner,
        PtySubmitEntry::Direct,
    ] {
        let mut submit_calls = 0;
        let result = submit_entry(entry, "shared body", |text| {
            submit_calls += 1;
            assert_eq!(text, "shared body");
            Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                waited: Duration::ZERO,
                screen_tail: "> shared body".into(),
            })
        });
        assert!(matches!(
            result,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
        assert_eq!(submit_calls, 1);
    }
}
