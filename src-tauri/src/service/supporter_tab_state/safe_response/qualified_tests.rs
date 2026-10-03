use super::*;
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionQuestion, DecisionQuestionsRequest, DecisionResponse, TokenMode,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const STAGE_TIMEOUT: Duration = Duration::from_secs(15);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_STAGE_SCREEN_BYTES: usize = 8 * 1024;
const MAX_MODEL_SCREENS: usize = 4;
const MAX_MODEL_SCREEN_BYTES: usize = 8 * 1024;
const MAX_JOURNAL_BYTES: usize = 64 * 1024;
const INITIAL_MARKER: &str = "SAFE_TEST_INITIAL_MODAL";
const FOCUS_MARKER: &str = "SAFE_TEST_FOCUS_EVENT";
const CLOSED_MARKER: &str = "SAFE_TEST_MODAL_CLOSED";
const SESSION_ID: &str = "fixture-session-private";
const SESSION_NONCE: &str = "fixture-nonce-private";
const JOURNAL_FIELDS: &str = "phase|observation_id|decision_point|observation_status|decided_by|classification|model_verdict|alternate_verdict|candidate_evidence|correlation";
const CORRELATION_FIELDS: &str = "version|decision_point|observation_id|launch_id|session_id|occurrence_id|producer|parent_observation_id|frame_hash|candidate_evidence";

struct StaticModalModel {
    model_id: String,
    screens: Arc<Mutex<CapturedScreens>>,
}
#[derive(Clone, Debug, Default)]
struct CapturedScreens {
    count: usize,
    snapshots: Vec<BoundedText>,
    truncated: bool,
}
impl CapturedScreens {
    fn record(&mut self, screen: &str) {
        self.count += 1;
        if self.snapshots.len() == MAX_MODEL_SCREENS {
            self.truncated = true;
        } else {
            self.snapshots
                .push(BoundedText::tail(screen, MAX_MODEL_SCREEN_BYTES));
        }
    }
}
impl DecisionModel for StaticModalModel {
    fn supports_question_batch(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        "static-modal-fixture"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn model_id(&self) -> Option<String> {
        Some(self.model_id.clone())
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: false,
        }
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        vec![DecisionModelInfo {
            id: self.model_id.clone(),
            display_name: "Fixture decision model".into(),
        }]
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        self.screens
            .lock()
            .record(request.state["screen"].as_str().unwrap_or_default());
        let mut answers = BTreeMap::new();
        for (name, question) in &request.questions {
            let answer = match &question.question {
                DecisionQuestion::Noul { .. } => DecisionAnswer::Noul { p: 0.99 },
                DecisionQuestion::Choice { options } => {
                    let choice = match name.as_str() {
                        "label" => LOGIN_OR_AUTH_REQUIRED,
                        "input_area" => "active_modal",
                        "safe_choice" => "2",
                        _ => return Err(DecisionError::Unsupported),
                    };
                    if !options.contains_key(choice) {
                        return Err(DecisionError::Parse("fixture choice is unavailable".into()));
                    }
                    DecisionAnswer::Choice {
                        choice: choice.into(),
                        probabilities: BTreeMap::from([(choice.into(), 0.99)]),
                        confidence: Some(0.99),
                    }
                }
                DecisionQuestion::Score { .. } => return Err(DecisionError::Unsupported),
            };
            answers.insert(name.clone(), answer);
        }
        request.validate_answers(&answers)?;
        Ok(DecisionResponse {
            answers,
            usage: None,
        })
    }
    fn decide(
        &self,
        _: &codeterm_types::decision::DecisionRequest,
        _: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundedText {
    text: String,
    truncated: bool,
}
impl BoundedText {
    fn tail(text: &str, max_bytes: usize) -> Self {
        if text.len() <= max_bytes {
            return Self {
                text: text.to_string(),
                truncated: false,
            };
        }
        let prefix = "[truncated]\n";
        let content_limit = max_bytes.saturating_sub(prefix.len());
        let mut start = text.len().saturating_sub(content_limit);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        Self {
            text: format!("{prefix}{}", &text[start..]),
            truncated: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalStage {
    Initial,
    Focus,
    Closed,
}
impl ModalStage {
    const ALL: [Self; 3] = [Self::Initial, Self::Focus, Self::Closed];
    fn marker(self) -> &'static str {
        match self {
            Self::Initial => INITIAL_MARKER,
            Self::Focus => FOCUS_MARKER,
            Self::Closed => CLOSED_MARKER,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReaderEnd {
    Eof,
    Panicked,
    ReadError {
        kind: String,
        raw_os_error: Option<i32>,
    },
    SpawnError {
        kind: String,
    },
}
impl ReaderEnd {
    fn is_clean_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }
}
#[derive(Clone, Debug)]
struct ScreenSnapshot {
    sequence: u64,
    observed_at: Instant,
    screen: BoundedText,
}
#[derive(Clone, Debug)]
struct StageEvent {
    stage: ModalStage,
    snapshot: ScreenSnapshot,
}
#[derive(Clone, Debug)]
struct ReaderEvidence {
    started_at: Instant,
    sequence: u64,
    last_snapshot: Option<ScreenSnapshot>,
    stage_events: Vec<StageEvent>,
    end: Option<ReaderEnd>,
    end_elapsed_ms: Option<u128>,
    reader_finished: bool,
}
impl ReaderEvidence {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            sequence: 0,
            last_snapshot: None,
            stage_events: Vec::new(),
            end: None,
            end_elapsed_ms: None,
            reader_finished: false,
        }
    }
    fn record_screen(&mut self, screen: &str) {
        self.sequence += 1;
        let snapshot = ScreenSnapshot {
            sequence: self.sequence,
            observed_at: Instant::now(),
            screen: BoundedText::tail(screen, MAX_STAGE_SCREEN_BYTES),
        };
        self.last_snapshot = Some(snapshot.clone());
        for stage in ModalStage::ALL {
            if screen.contains(stage.marker())
                && !self.stage_events.iter().any(|event| event.stage == stage)
            {
                self.stage_events.push(StageEvent {
                    stage,
                    snapshot: snapshot.clone(),
                });
            }
        }
    }
    fn record_end(&mut self, end: ReaderEnd) {
        self.end_elapsed_ms = Some(self.started_at.elapsed().as_millis());
        self.end = Some(end);
    }
    fn mark_finished(&mut self) {
        self.reader_finished = true;
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum StageFailureReason {
    Deadline,
    ReaderEof,
    ReaderError(ReaderEnd),
    NotificationDisconnected,
}
#[derive(Clone, Debug)]
struct StageFailure {
    stage: ModalStage,
    marker: &'static str,
    reason: StageFailureReason,
    elapsed_ms: u128,
    deadline_ms: u128,
    event_sequence: u64,
    last_snapshot: Option<ScreenSnapshot>,
}
fn stage_failure(
    stage: ModalStage,
    reason: StageFailureReason,
    elapsed_ms: u128,
    evidence: &ReaderEvidence,
) -> StageFailure {
    StageFailure {
        stage,
        marker: stage.marker(),
        reason,
        elapsed_ms,
        deadline_ms: STAGE_TIMEOUT.as_millis(),
        event_sequence: evidence.sequence,
        last_snapshot: evidence.last_snapshot.clone(),
    }
}
fn wait_for_screen(
    evidence: &Arc<Mutex<ReaderEvidence>>,
    wake: &mpsc::Receiver<()>,
    stage: ModalStage,
) -> Result<StageEvent, StageFailure> {
    let started_at = Instant::now();
    let deadline = started_at + STAGE_TIMEOUT;
    let fail = |reason, evidence: &ReaderEvidence| {
        stage_failure(stage, reason, started_at.elapsed().as_millis(), evidence)
    };
    loop {
        let snapshot = evidence.lock().clone();
        if let Some(event) = snapshot
            .stage_events
            .iter()
            .find(|event| event.stage == stage)
            .cloned()
        {
            if event.snapshot.observed_at <= deadline {
                return Ok(event);
            }
            return Err(fail(StageFailureReason::Deadline, &snapshot));
        }
        if let Some(end) = snapshot.end.clone() {
            let reason = if end == ReaderEnd::Eof {
                StageFailureReason::ReaderEof
            } else {
                StageFailureReason::ReaderError(end)
            };
            return Err(fail(reason, &snapshot));
        }
        if snapshot.reader_finished {
            return Err(fail(
                StageFailureReason::NotificationDisconnected,
                &snapshot,
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(fail(StageFailureReason::Deadline, &snapshot));
        }
        match wake.recv_timeout(remaining) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let snapshot = evidence.lock().clone();
                return Err(fail(
                    StageFailureReason::NotificationDisconnected,
                    &snapshot,
                ));
            }
        }
    }
}
struct ReaderFinish {
    session: Arc<codeterm_pty::pty::manager::PtySession>,
    evidence: Arc<Mutex<ReaderEvidence>>,
    wake: mpsc::SyncSender<()>,
}
impl Drop for ReaderFinish {
    fn drop(&mut self) {
        self.session.mark_backend_reader_finished();
        let mut evidence = self.evidence.lock();
        if std::thread::panicking() && evidence.end.is_none() {
            evidence.record_end(ReaderEnd::Panicked);
        }
        evidence.mark_finished();
        let _ = self.wake.try_send(());
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReaderWaitStatus {
    Completed,
    Late,
    Deadline,
    NotificationDisconnected,
}
#[derive(Clone, Debug)]
struct ReaderWait {
    end: Option<ReaderEnd>,
    status: ReaderWaitStatus,
    observed_elapsed_ms: Option<u128>,
}
fn reader_wait(
    end: Option<ReaderEnd>,
    status: ReaderWaitStatus,
    started_at: Instant,
) -> ReaderWait {
    ReaderWait {
        end,
        status,
        observed_elapsed_ms: Some(started_at.elapsed().as_millis()),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReaderJoinStatus {
    Joined,
    Panicked(String),
    NotAttempted,
    Deadline,
}
#[derive(Clone, Debug)]
struct CompletionEvidence {
    child_exit: Option<Option<i32>>,
    child_exit_observed_ms: Option<u128>,
    reader: ReaderWait,
    reader_join: ReaderJoinStatus,
    within_deadline: bool,
    elapsed_ms: u128,
}
impl CompletionEvidence {
    fn succeeds(&self) -> bool {
        let limit = COMPLETION_TIMEOUT.as_millis();
        child_exit_is_success(self.child_exit)
            && self
                .reader
                .end
                .as_ref()
                .is_some_and(ReaderEnd::is_clean_eof)
            && self.reader.status == ReaderWaitStatus::Completed
            && self.reader_join == ReaderJoinStatus::Joined
            && self.within_deadline
            && self.elapsed_ms <= limit
            && self
                .child_exit_observed_ms
                .is_some_and(|elapsed| elapsed <= limit)
            && self
                .reader
                .observed_elapsed_ms
                .is_some_and(|elapsed| elapsed <= limit)
    }
}
#[derive(Clone, Debug)]
struct CleanupEvidence {
    generation_matched: bool,
    begin_kill_error: Option<String>,
    reap_error: Option<String>,
    reaped: bool,
    post_cleanup_child_exit: Option<Option<i32>>,
    post_cleanup_reader_end: Option<ReaderEnd>,
    reader_join: ReaderJoinStatus,
    reader_started: bool,
}
impl CleanupEvidence {
    fn succeeds(&self) -> bool {
        self.generation_matched
            && self.begin_kill_error.is_none()
            && self.reap_error.is_none()
            && self.reaped
            && !matches!(
                &self.reader_join,
                ReaderJoinStatus::Panicked(_) | ReaderJoinStatus::Deadline
            )
            && (!self.reader_started
                || (self.post_cleanup_reader_end.is_some()
                    && self.reader_join == ReaderJoinStatus::Joined))
    }
}
struct PtyChild {
    service: crate::service::ServiceContext,
    pty: String,
    tab_id: String,
    session: Arc<codeterm_pty::pty::manager::PtySession>,
    generation: u64,
    reader_evidence: Arc<Mutex<ReaderEvidence>>,
    wake: mpsc::Receiver<()>,
    wake_sender: Option<mpsc::SyncSender<()>>,
    reader_join: Option<JoinHandle<()>>,
    reader_started: bool,
    completion_deadline: Option<Instant>,
    completion: Option<CompletionEvidence>,
    cleanup_started: bool,
}
impl PtyChild {
    fn new(
        service: crate::service::ServiceContext,
        pty: String,
        tab_id: String,
        session: Arc<codeterm_pty::pty::manager::PtySession>,
    ) -> Self {
        let generation = session.generation;
        let reader_evidence = Arc::new(Mutex::new(ReaderEvidence::new()));
        let (wake_sender, wake) = mpsc::sync_channel(1);
        Self {
            service,
            pty,
            tab_id,
            session,
            generation,
            reader_evidence,
            wake,
            wake_sender: Some(wake_sender),
            reader_join: None,
            reader_started: false,
            completion_deadline: None,
            completion: None,
            cleanup_started: false,
        }
    }
    fn start_reader(&mut self) -> Result<(), String> {
        let mut reader = self
            .session
            .reader
            .lock()
            .map_err(|error| error.to_string())?
            .take()
            .ok_or_else(|| "fixture PTY reader was already taken".to_string())?;
        self.session.mark_backend_reader_started();
        self.reader_started = true;
        let session = self.session.clone();
        let service = self.service.clone();
        let pty = self.pty.clone();
        let generation = self.generation;
        let evidence = self.reader_evidence.clone();
        let wake = self.wake_sender.take().unwrap();
        let reader_join = std::thread::Builder::new()
            .name("qualified-modal-pty-reader".into())
            .spawn(move || {
                let _finish = ReaderFinish {
                    session,
                    evidence: evidence.clone(),
                    wake: wake.clone(),
                };
                let end = loop {
                    let mut bytes = [0u8; 8192];
                    match reader.read(&mut bytes) {
                        Ok(0) => break ReaderEnd::Eof,
                        Ok(count) => {
                            service.feed_pty_output_for_generation(
                                &pty,
                                generation,
                                &bytes[..count],
                            );
                            let screen = service
                                .screen_state
                                .lock()
                                .get_screen_text(&pty)
                                .unwrap_or_default();
                            evidence.lock().record_screen(&screen);
                            let _ = wake.try_send(());
                        }
                        Err(error) => {
                            break ReaderEnd::ReadError {
                                kind: format!("{:?}", error.kind()),
                                raw_os_error: error.raw_os_error(),
                            }
                        }
                    }
                };
                evidence.lock().record_end(end);
            })
            .map_err(|error| {
                self.session.mark_backend_reader_finished();
                self.reader_evidence
                    .lock()
                    .record_end(ReaderEnd::SpawnError {
                        kind: format!("{:?}", error.kind()),
                    });
                self.reader_evidence.lock().mark_finished();
                format!("fixture PTY reader spawn failed: {:?}", error.kind())
            })?;
        self.reader_join = Some(reader_join);
        Ok(())
    }
    fn wait_for_screen(&self, stage: ModalStage) -> Result<StageEvent, StageFailure> {
        wait_for_screen(&self.reader_evidence, &self.wake, stage)
    }
    fn completion_deadline(&mut self) -> Instant {
        *self
            .completion_deadline
            .get_or_insert_with(|| Instant::now() + COMPLETION_TIMEOUT)
    }
    fn wait_for_reader_end(&self, deadline: Instant, started_at: Instant) -> ReaderWait {
        loop {
            let snapshot = self.reader_evidence.lock().clone();
            if snapshot.reader_finished {
                let status = if snapshot.end.is_none() {
                    ReaderWaitStatus::NotificationDisconnected
                } else if Instant::now() <= deadline {
                    ReaderWaitStatus::Completed
                } else {
                    ReaderWaitStatus::Late
                };
                return reader_wait(snapshot.end, status, started_at);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return reader_wait(snapshot.end, ReaderWaitStatus::Deadline, started_at);
            }
            match self.wake.recv_timeout(remaining) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return reader_wait(
                        self.reader_evidence.lock().end.clone(),
                        ReaderWaitStatus::Deadline,
                        started_at,
                    );
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return reader_wait(
                        self.reader_evidence.lock().end.clone(),
                        ReaderWaitStatus::NotificationDisconnected,
                        started_at,
                    )
                }
            }
        }
    }
    fn join_reader(&mut self, allowed: bool, deadline: Instant) -> ReaderJoinStatus {
        if !allowed {
            return if Instant::now() <= deadline {
                ReaderJoinStatus::NotAttempted
            } else {
                ReaderJoinStatus::Deadline
            };
        }
        if Instant::now() > deadline {
            return ReaderJoinStatus::Deadline;
        }
        let Some(reader_join) = self.reader_join.take() else {
            return ReaderJoinStatus::NotAttempted;
        };
        match reader_join.join() {
            Ok(()) if Instant::now() <= deadline => ReaderJoinStatus::Joined,
            Ok(()) => ReaderJoinStatus::Deadline,
            Err(payload) => ReaderJoinStatus::Panicked(panic_message(payload.as_ref())),
        }
    }
    fn await_normal_completion(&mut self) -> Result<(), String> {
        let started_at = Instant::now();
        let deadline = self.completion_deadline();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let child_exit = self.session.child_exit.wait_resolved(remaining);
        let child_exit_observed_at = Instant::now();
        let child_exit_observed_ms = Some(
            child_exit_observed_at
                .duration_since(started_at)
                .as_millis(),
        );
        let reader = self.wait_for_reader_end(deadline, started_at);
        let reader_join = self.join_reader(
            self.reader_evidence.lock().reader_finished
                || reader.status == ReaderWaitStatus::NotificationDisconnected,
            deadline,
        );
        let evidence = CompletionEvidence {
            child_exit,
            child_exit_observed_ms,
            reader,
            reader_join,
            within_deadline: Instant::now() <= deadline,
            elapsed_ms: Instant::now().duration_since(started_at).as_millis(),
        };
        let success = evidence.succeeds();
        self.completion = Some(evidence.clone());
        if success {
            Ok(())
        } else {
            Err(format!(
                "owned host/reader completion was not clean: {evidence:?}"
            ))
        }
    }
    fn cleanup(&mut self, deadline: Instant) -> CleanupEvidence {
        let (generation_matched, begin_kill) = {
            let mut manager = self.service.pty.lock();
            let matched = manager.session_generation_matches(&self.pty, self.generation);
            let result = if matched {
                manager
                    .begin_kill(&self.pty)
                    .map_err(|error| error.to_string())
            } else {
                Err("fixture PTY generation is no longer the manager owner".to_string())
            };
            (matched, result)
        };
        let (begin_kill_error, reap_error, reaped) = match begin_kill {
            Ok(reaper) => {
                let result =
                    reaper.wait_bounded(deadline.saturating_duration_since(Instant::now()));
                match result {
                    Ok(()) if Instant::now() <= deadline => (None, None, true),
                    Ok(()) => (
                        None,
                        Some("PTY reaper completed after the shared deadline".to_string()),
                        false,
                    ),
                    Err(error) => (None, Some(error), false),
                }
            }
            Err(error) => (Some(error), None, false),
        };
        let reader_finished = self.reader_evidence.lock().reader_finished || reaped;
        let reader_join = self.join_reader(reader_finished, deadline);
        let evidence = CleanupEvidence {
            generation_matched,
            begin_kill_error,
            reap_error,
            reaped,
            post_cleanup_child_exit: self.session.child_exit.resolved(),
            post_cleanup_reader_end: self.reader_evidence.lock().end.clone(),
            reader_join,
            reader_started: self.reader_started,
        };
        self.cleanup_started = true;
        evidence
    }
}
impl Drop for PtyChild {
    fn drop(&mut self) {
        if self.cleanup_started {
            return;
        }
        self.cleanup_started = true;
        let result = {
            let mut manager = self.service.pty.lock();
            if manager.session_generation_matches(&self.pty, self.generation) {
                manager
                    .begin_kill(&self.pty)
                    .map_err(|error| error.to_string())
            } else {
                Err("fixture PTY generation is no longer the manager owner".to_string())
            }
        };
        match result {
            Ok(reaper) => eprintln!(
                "qualified fixture fallback cleanup: {:?}",
                reaper.wait_bounded(Duration::ZERO)
            ),
            Err(error) => eprintln!("qualified fixture fallback cleanup failed: {error}"),
        }
    }
}
#[derive(Clone, Debug)]
struct JournalEvidence {
    byte_count: usize,
    complete: bool,
    truncated: bool,
    partial_tail: bool,
    parse_errors: Vec<String>,
    read_error: Option<String>,
    rows: Vec<serde_json::Value>,
    missing_expected_rows: Option<Vec<String>>,
    contains_private_identity: bool,
}
fn journal_row(value: &serde_json::Value) -> Option<serde_json::Value> {
    let mut row = value.as_object()?.clone();
    row.retain(|key, _| JOURNAL_FIELDS.split('|').any(|field| field == key.as_str()));
    if let Some(correlation) = row
        .get_mut("correlation")
        .and_then(serde_json::Value::as_object_mut)
    {
        correlation.retain(|key, _| {
            CORRELATION_FIELDS
                .split('|')
                .any(|field| field == key.as_str())
        });
    }
    Some(serde_json::Value::Object(row))
}
fn missing_expected_rows(rows: &[serde_json::Value]) -> Vec<String> {
    let mut missing = Vec::new();
    if !rows
        .iter()
        .any(|row| row["correlation"]["producer"] == "prompt_established")
    {
        missing.push("prompt_established".into());
    }
    if !rows
        .iter()
        .any(|row| row["correlation"]["producer"] == "prompt_confirmation")
    {
        missing.push("prompt_confirmation".into());
    }
    if !rows.iter().any(|row| row["phase"] == "action_authorized") {
        missing.push("action_authorized".into());
    }
    missing
}
fn parse_journal_snapshot(
    mut bytes: Vec<u8>,
    truncated: bool,
    read_error: Option<String>,
) -> JournalEvidence {
    let byte_count = bytes.len();
    let raw = String::from_utf8_lossy(&bytes);
    let contains_private_identity = raw.contains(SESSION_ID) || raw.contains(SESSION_NONCE);
    let partial_tail = !bytes.is_empty() && !bytes.ends_with(b"\n");
    let mut rows = Vec::new();
    let mut parse_errors = Vec::new();
    match String::from_utf8(std::mem::take(&mut bytes)) {
        Ok(text) => {
            for (index, line) in text.lines().enumerate() {
                match serde_json::from_str::<serde_json::Value>(line) {
                    Ok(value) => match journal_row(&value) {
                        Some(row) => rows.push(row),
                        None => {
                            parse_errors.push(format!("line {}: row was not an object", index + 1))
                        }
                    },
                    Err(error) => parse_errors.push(format!("line {}: {error}", index + 1)),
                }
            }
        }
        Err(error) => parse_errors.push(format!(
            "journal was not UTF-8 at byte {}",
            error.utf8_error().valid_up_to()
        )),
    }
    let complete = !truncated && !partial_tail && parse_errors.is_empty() && read_error.is_none();
    let missing_expected_rows = complete.then(|| missing_expected_rows(&rows));
    JournalEvidence {
        byte_count,
        complete,
        truncated,
        partial_tail,
        parse_errors,
        read_error,
        rows,
        missing_expected_rows,
        contains_private_identity,
    }
}
fn capture_journal(path: &Path) -> JournalEvidence {
    let mut bytes = Vec::new();
    let result = std::fs::File::open(path).and_then(|file| {
        file.take((MAX_JOURNAL_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
    });
    let read_error = result
        .err()
        .map(|error| format!("journal read/open failed: {:?}", error.kind()));
    let truncated = bytes.len() > MAX_JOURNAL_BYTES;
    bytes.truncate(MAX_JOURNAL_BYTES);
    parse_journal_snapshot(bytes, truncated, read_error)
}
#[derive(Clone, Debug)]
struct ModalEvidence {
    reader: ReaderEvidence,
    reader_started: bool,
    captured_screens: CapturedScreens,
    identity: Option<(bool, bool, bool, bool)>,
    manager_generation_matches: bool,
    child_exit_before_cleanup: Option<Option<i32>>,
    journal: JournalEvidence,
}
fn capture_modal_evidence(
    child: &PtyChild,
    screens: &Arc<Mutex<CapturedScreens>>,
    journal_path: &Path,
) -> ModalEvidence {
    let reader = child.reader_evidence.lock().clone();
    let identity = child.service.mux.read().get_tab(&child.tab_id).map(|tab| {
        (
            tab.pending_prompt.is_none(),
            tab.agent_session_id.as_deref() == Some(SESSION_ID),
            tab.session_binding_nonce.as_deref() == Some(SESSION_NONCE)
                && tab.reported_session_binding_nonce.as_deref() == Some(SESSION_NONCE),
            tab.pty_id.as_deref() == Some(child.pty.as_str()),
        )
    });
    let manager_generation_matches = child
        .service
        .pty
        .lock()
        .session_generation_matches(&child.pty, child.generation);
    ModalEvidence {
        reader,
        reader_started: child.reader_started,
        captured_screens: screens.lock().clone(),
        identity,
        manager_generation_matches,
        child_exit_before_cleanup: child.session.child_exit.resolved(),
        journal: capture_journal(journal_path),
    }
}
macro_rules! reject_unless {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err($message.into());
        }
    };
}
fn verify_qualified_journey(
    evidence: &ModalEvidence,
    completion: Option<&CompletionEvidence>,
) -> Result<(), String> {
    let reader = &evidence.reader;
    for stage in ModalStage::ALL {
        if !reader
            .stage_events
            .iter()
            .any(|event| event.stage == stage && event.snapshot.sequence <= reader.sequence)
        {
            return Err(format!("fixture did not reach stage {stage:?}"));
        }
    }
    let captures = &evidence.captured_screens;
    let repeated = captures.snapshots.iter().enumerate().any(|(i, left)| {
        captures.snapshots[i + 1..]
            .iter()
            .any(|right| left.text == right.text)
    });
    let model_screens_repeat = captures.count >= 2
        && !captures.truncated
        && !captures.snapshots.iter().any(|s| s.truncated)
        && repeated;
    let current_identity =
        evidence.identity == Some((true, true, true, true)) && evidence.manager_generation_matches;
    let completion_matches = completion.is_some_and(CompletionEvidence::succeeds)
        && completion.is_some_and(|c| c.child_exit == evidence.child_exit_before_cleanup);
    reject_unless!(
        evidence.reader_started
            && reader.reader_finished
            && current_identity
            && model_screens_repeat
            && completion_matches,
        "stage, model, identity, and host completion evidence was incomplete"
    );
    if !evidence.journal.complete {
        return Err(format!(
            "fixture journal snapshot is incomplete: bytes={} truncated={} partial_tail={} parse_errors={:?} read_error={:?}",
            evidence.journal.byte_count,
            evidence.journal.truncated,
            evidence.journal.partial_tail,
            evidence.journal.parse_errors,
            evidence.journal.read_error
        ));
    }
    reject_unless!(
        !evidence.journal.contains_private_identity,
        "journal contains private fixture identity"
    );
    if evidence
        .journal
        .missing_expected_rows
        .as_ref()
        .is_some_and(|rows| !rows.is_empty())
    {
        return Err(format!(
            "fixture journal lacks required rows: {:?}",
            evidence.journal.missing_expected_rows
        ));
    }
    let rows = &evidence.journal.rows;
    let seed = rows
        .iter()
        .find(|row| row["correlation"]["producer"] == "prompt_established")
        .ok_or_else(|| "prompt-established capture row is absent".to_string())?;
    let confirmation = rows
        .iter()
        .find(|row| row["correlation"]["producer"] == "prompt_confirmation")
        .ok_or_else(|| "prompt-confirmation capture row is absent".to_string())?;
    if seed["observation_status"] != "completed"
        || confirmation["observation_status"] != "completed"
        || seed["decision_point"] != "prompt_candidate"
        || confirmation["decision_point"] != "prompt_candidate"
        || seed["decided_by"] != "adapter"
        || confirmation["decided_by"] != "adapter"
        || seed["model_verdict"].is_null()
        || confirmation["model_verdict"].is_null()
        || seed["correlation"]["occurrence_id"] != confirmation["correlation"]["occurrence_id"]
        || seed["correlation"]["frame_hash"] != confirmation["correlation"]["frame_hash"]
        || seed["observation_id"] == confirmation["observation_id"]
        || confirmation["correlation"]["parent_observation_id"] != seed["observation_id"]
    {
        return Err(format!(
            "seed/confirmation evidence mismatch: seed={seed:?} confirmation={confirmation:?}"
        ));
    }
    let action = rows
        .iter()
        .find(|row| {
            row["phase"] == "action_authorized"
                && row["observation_id"] == confirmation["observation_id"]
        })
        .ok_or_else(|| "confirmation action row is absent".to_string())?;
    reject_unless!(
        action["correlation"] == confirmation["correlation"],
        "action lost confirmation correlation"
    );
    Ok(())
}
fn run_modal(child: &mut PtyChild, command: &str) -> Result<(), String> {
    let bound = child
        .service
        .screen_state
        .lock()
        .bind_pty_generation(&child.pty, child.generation);
    if !bound {
        return Err("fixture screen binding rejected its PTY generation".to_string());
    }
    child.start_reader()?;
    child
        .service
        .pty
        .lock()
        .write_and_flush(&child.pty, command.as_bytes(), Duration::from_secs(2))
        .map_err(|error| format!("fixture command write failed: {error}"))?;
    for stage in [ModalStage::Initial, ModalStage::Focus, ModalStage::Closed] {
        let event = child
            .wait_for_screen(stage)
            .map_err(|failure| format!("fixture stage failed: {failure:?}"))?;
        let screen = &event.snapshot.screen.text;
        if stage == ModalStage::Closed
            && (!screen.contains("SAFE_TEST_OPTION_SELECTED")
                || screen.contains("SAFE_TEST_WRONG_RESPONSE"))
        {
            return Err("fixture closed without the exact safe option".to_string());
        }
    }
    child.await_normal_completion()
}
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|message| (*message).into())
        })
        .unwrap_or_else(|| "fixture run panicked with a non-string payload".into())
}
#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_safe_response_qualified_static_onboarding_planner_completion() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty = format!("static-modal-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty.clone());
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some(SESSION_ID.into());
        tab.session_binding_nonce = Some(SESSION_NONCE.into());
        tab.reported_session_binding_nonce = Some(SESSION_NONCE.into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.role = Some(codeterm_types::agent::AgentRole::Manager);
        tab.role_profile = Some("planner".into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));
    let adapter = format!("fixture-{}", uuid::Uuid::new_v4().simple());
    let model_id = format!("fixture-model-{}", uuid::Uuid::new_v4().simple());
    let config = SupporterConfig {
        adapter: Some(adapter.clone()),
        connections: BTreeMap::from([(
            adapter.clone(),
            codeterm_types::config::SupporterConnection {
                model: Some(model_id.clone()),
                endpoint: None,
            },
        )]),
        ..Default::default()
    };
    let screens = Arc::new(Mutex::new(CapturedScreens::default()));
    fixture.service.supporter_models.register(
        &adapter,
        Arc::new(StaticModalModel {
            model_id: model_id.clone(),
            screens: screens.clone(),
        }),
    );
    let runtime = Arc::new(crate::service::supporter::SupporterRuntime::new(
        fixture.temp.path().join("supporter"),
    ));
    let _config_guard = fixture
        .service
        .override_supporter_decision_for_test(config, runtime);
    let _qualification_guard = override_qualification_for_test(
        &adapter,
        &model_id,
        &criteria_version(),
        serde_json::json!({ "fixture_authorized": true }),
    );
    let initial = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let focused = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-not-now.txt");
    std::fs::write(fixture.temp.path().join("initial.txt"), initial).unwrap();
    std::fs::write(fixture.temp.path().join("focused.txt"), focused).unwrap();
    let script = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n\
[Console]::WriteLine('SAFE_TEST_INITIAL_MODAL')\n\
[Console]::Write([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'initial.txt'), [System.Text.Encoding]::UTF8))\n\
$first = [Console]::ReadKey($true)\n\
[Console]::WriteLine('SAFE_TEST_FOCUS_EVENT')\n\
[Console]::Write([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'focused.txt'), [System.Text.Encoding]::UTF8))\n\
$second = [Console]::ReadKey($true)\n\
if (($first.KeyChar -eq 'j') -and ($second.Key -eq [System.ConsoleKey]::Enter)) { [Console]::WriteLine('SAFE_TEST_OPTION_SELECTED'); [Console]::WriteLine('SAFE_TEST_MODAL_CLOSED'); exit 0 }\n\
[Console]::WriteLine('SAFE_TEST_WRONG_RESPONSE'); exit 9\n";
    let script_path = fixture.temp.path().join("modal.ps1");
    std::fs::write(&script_path, script).unwrap();
    let command_path = script_path.to_string_lossy().replace('\'', "''");
    let command = format!(
        "$global:LASTEXITCODE = 98; $ErrorActionPreference = 'Stop'; try {{ & '{command_path}'; $scriptExit = $global:LASTEXITCODE; if ($scriptExit -eq 98) {{ throw 'fixture script returned without an exit code' }}; [Console]::Out.Flush(); [Console]::Error.Flush(); [System.Environment]::Exit([int]$scriptExit) }} catch {{ try {{ [Console]::Error.WriteLine('SAFE_TEST_SETUP_FAILURE: {{0}}', $_.Exception.Message); [Console]::Error.Flush() }} finally {{ [System.Environment]::Exit(98) }} }}\r"
    );
    let (_, session) = fixture
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some(pty.clone()),
            Some("powershell.exe".into()),
            None,
            120,
            40,
            Default::default(),
        )
        .expect("spawn the modal fixture child in the ServiceContext PTY");
    let mut child = PtyChild::new(fixture.service.clone(), pty, tab_id, session);
    let journal_path = fixture.temp.path().join("supporter/journal.jsonl");
    let primary = catch_unwind(AssertUnwindSafe(|| run_modal(&mut child, &command)));
    let primary = match primary {
        Ok(result) => result,
        Err(payload) => Err(panic_message(payload.as_ref())),
    };
    let evidence = capture_modal_evidence(&child, &screens, &journal_path);
    let validation = primary
        .as_ref()
        .ok()
        .map(|_| verify_qualified_journey(&evidence, child.completion.as_ref()));
    let completion_deadline = child.completion_deadline();
    let cleanup = child.cleanup(completion_deadline);
    assert!(
        primary.is_ok()
            && validation.as_ref().is_some_and(Result::is_ok)
            && cleanup.succeeds(),
        "qualified fixture did not complete cleanly\nprimary={primary:?}\nvalidation={validation:?}\nevidence={evidence:#?}\ncleanup={cleanup:#?}"
    );
}
fn child_exit_is_success(exit: Option<Option<i32>>) -> bool {
    exit == Some(Some(0))
}
#[test]
fn qualified_fixture_stage_timeout_retains_actual_last_snapshot() {
    let mut evidence = ReaderEvidence::new();
    evidence.record_screen("fixture output without a stage marker");
    let failure = stage_failure(
        ModalStage::Focus,
        StageFailureReason::Deadline,
        STAGE_TIMEOUT.as_millis(),
        &evidence,
    );
    assert_eq!(
        (
            failure.stage,
            failure.marker,
            failure.reason,
            failure.elapsed_ms,
            failure.deadline_ms,
            failure.event_sequence,
            failure
                .last_snapshot
                .as_ref()
                .map(|s| s.screen.text.as_str())
        ),
        (
            ModalStage::Focus,
            FOCUS_MARKER,
            StageFailureReason::Deadline,
            STAGE_TIMEOUT.as_millis(),
            15_000,
            1,
            Some("fixture output without a stage marker")
        )
    );
}
#[test]
fn qualified_fixture_reader_end_distinguishes_eof_from_read_error() {
    assert!(ReaderEnd::Eof.is_clean_eof());
    assert!(!ReaderEnd::ReadError {
        kind: "BrokenPipe".into(),
        raw_os_error: Some(109)
    }
    .is_clean_eof());
    assert!(!ReaderEnd::SpawnError {
        kind: "WouldBlock".into()
    }
    .is_clean_eof());
    assert!(!ReaderEnd::Panicked.is_clean_eof());
}
#[test]
fn qualified_fixture_unobservable_and_nonzero_exit_refuse_success() {
    assert!([None, Some(None), Some(Some(9))]
        .into_iter()
        .all(|exit| !child_exit_is_success(exit)));
    assert!(child_exit_is_success(Some(Some(0))));
}
#[test]
fn qualified_fixture_cleanup_exit_cannot_promote_pre_cleanup_failure() {
    let cleanup_exit = Some(Some(0));
    let completion = CompletionEvidence {
        child_exit: None,
        child_exit_observed_ms: None,
        reader: ReaderWait {
            end: None,
            status: ReaderWaitStatus::Deadline,
            observed_elapsed_ms: Some(COMPLETION_TIMEOUT.as_millis()),
        },
        reader_join: ReaderJoinStatus::NotAttempted,
        within_deadline: false,
        elapsed_ms: COMPLETION_TIMEOUT.as_millis(),
    };
    assert!(child_exit_is_success(cleanup_exit));
    assert!(!completion.succeeds());
}
#[test]
fn qualified_fixture_incomplete_journal_does_not_claim_missing_rows() {
    let evidence = parse_journal_snapshot(
        br#"{"phase":"observation_completed","observation_id":"capture-1"}"#.to_vec(),
        false,
        None,
    );
    assert!(!evidence.complete);
    assert!(evidence.partial_tail);
    assert!(evidence.missing_expected_rows.is_none());
    assert_eq!(evidence.rows.len(), 1);
    let malformed = parse_journal_snapshot(b"{\n".to_vec(), false, None);
    assert!(
        !malformed.complete
            && malformed.parse_errors.len() == 1
            && malformed.parse_errors[0].contains("line 1")
            && malformed.missing_expected_rows.is_none()
    );
}
