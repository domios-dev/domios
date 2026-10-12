use super::{
    BoundedText, CLOSED_MARKER, COMPLETION_TIMEOUT, FOCUS_MARKER, INITIAL_MARKER,
    MAX_FIXTURE_DIAGNOSTIC_BYTES, MAX_STAGE_SCREEN_BYTES, STAGE_TIMEOUT,
};
use parking_lot::Mutex;
use std::io::Read;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModalStage {
    Initial,
    Focus,
    Closed,
}
impl ModalStage {
    pub(super) const ALL: [Self; 3] = [Self::Initial, Self::Focus, Self::Closed];
    fn marker(self) -> &'static str {
        match self {
            Self::Initial => INITIAL_MARKER,
            Self::Focus => FOCUS_MARKER,
            Self::Closed => CLOSED_MARKER,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReaderEnd {
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
    pub(super) fn is_clean_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }
}
#[derive(Clone, Debug)]
pub(super) struct ScreenSnapshot {
    pub(super) sequence: u64,
    pub(super) observed_at: Instant,
    pub(super) screen: BoundedText,
}
#[derive(Clone, Debug)]
pub(super) struct StageEvent {
    pub(super) stage: ModalStage,
    pub(super) snapshot: ScreenSnapshot,
}
#[derive(Clone, Debug)]
pub(super) struct ReaderEvidence {
    started_at: Instant,
    pub(super) sequence: u64,
    pub(super) last_snapshot: Option<ScreenSnapshot>,
    pub(super) pending_output: Option<BoundedText>,
    pub(super) stage_events: Vec<StageEvent>,
    end: Option<ReaderEnd>,
    end_elapsed_ms: Option<u128>,
    pub(super) reader_finished: bool,
}
impl ReaderEvidence {
    pub(super) fn new() -> Self {
        Self {
            started_at: Instant::now(),
            sequence: 0,
            last_snapshot: None,
            pending_output: None,
            stage_events: Vec::new(),
            end: None,
            end_elapsed_ms: None,
            reader_finished: false,
        }
    }
    pub(super) fn record_pending_frame(&mut self, frame: &[u8]) {
        let safe = super::super::super::privacy::screen(&String::from_utf8_lossy(frame));
        self.pending_output = Some(BoundedText::tail(&safe, MAX_STAGE_SCREEN_BYTES));
    }
    pub(super) fn record_screen(&mut self, screen: &str) {
        self.pending_output = None;
        self.sequence += 1;
        let safe_screen = super::super::super::privacy::screen(screen);
        let snapshot = ScreenSnapshot {
            sequence: self.sequence,
            observed_at: Instant::now(),
            screen: BoundedText::tail(&safe_screen, MAX_STAGE_SCREEN_BYTES),
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
pub(super) enum StageFailureReason {
    Deadline,
    ReaderEof,
    ReaderError(ReaderEnd),
    NotificationDisconnected,
}
#[derive(Clone, Debug)]
pub(super) struct StageFailure {
    pub(super) stage: ModalStage,
    pub(super) marker: &'static str,
    pub(super) reason: StageFailureReason,
    pub(super) elapsed_ms: u128,
    pub(super) deadline_ms: u128,
    pub(super) event_sequence: u64,
    pub(super) last_snapshot: Option<ScreenSnapshot>,
    pub(super) pending_output: Option<BoundedText>,
}
pub(super) fn stage_failure(
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
        pending_output: evidence.pending_output.clone(),
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
pub(super) enum ReaderWaitStatus {
    Completed,
    Late,
    Deadline,
    NotificationDisconnected,
}
#[derive(Clone, Debug)]
pub(super) struct ReaderWait {
    pub(super) end: Option<ReaderEnd>,
    pub(super) status: ReaderWaitStatus,
    pub(super) observed_elapsed_ms: Option<u128>,
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
pub(super) enum ReaderJoinStatus {
    Joined,
    Panicked(String),
    NotAttempted,
    Deadline,
}
#[derive(Default)]
pub(super) struct ReaderJoinState {
    handle: Option<JoinHandle<()>>,
    outcome: Option<ReaderJoinStatus>,
}
impl ReaderJoinState {
    pub(super) fn from_handle(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
            outcome: None,
        }
    }
    pub(super) fn settle(&mut self, allowed: bool, deadline: Instant) -> ReaderJoinStatus {
        if let Some(outcome) = &self.outcome {
            return outcome.clone();
        }
        if !allowed {
            return ReaderJoinStatus::NotAttempted;
        }
        if Instant::now() > deadline {
            let outcome = ReaderJoinStatus::Deadline;
            self.outcome = Some(outcome.clone());
            return outcome;
        }
        let Some(handle) = self.handle.take() else {
            let outcome = ReaderJoinStatus::NotAttempted;
            self.outcome = Some(outcome.clone());
            return outcome;
        };
        let outcome = match handle.join() {
            Ok(()) if Instant::now() <= deadline => ReaderJoinStatus::Joined,
            Ok(()) => ReaderJoinStatus::Deadline,
            Err(payload) => ReaderJoinStatus::Panicked(panic_message(payload.as_ref())),
        };
        self.outcome = Some(outcome.clone());
        outcome
    }
}
#[derive(Clone, Debug)]
pub(super) struct CompletionEvidence {
    pub(super) child_exit: Option<Option<i32>>,
    pub(super) child_exit_observed_ms: Option<u128>,
    pub(super) reader: ReaderWait,
    pub(super) reader_join: ReaderJoinStatus,
    pub(super) within_deadline: bool,
    pub(super) elapsed_ms: u128,
}
impl CompletionEvidence {
    pub(super) fn succeeds(&self) -> bool {
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
#[derive(Clone)]
pub(super) struct CleanupEvidence {
    pub(super) generation_matched: bool,
    pub(super) begin_kill_error: Option<String>,
    pub(super) reap_error: Option<String>,
    pub(super) reaped: bool,
    pub(super) post_cleanup_child_exit: Option<Option<i32>>,
    pub(super) post_cleanup_reader_end: Option<ReaderEnd>,
    pub(super) reader_join: ReaderJoinStatus,
    pub(super) reader_started: bool,
}
impl std::fmt::Debug for CleanupEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CleanupEvidence")
            .field("generation_matched", &self.generation_matched)
            .field("begin_kill_error", &self.begin_kill_error)
            .field("reap_error", &self.reap_error)
            .field("reaped", &self.reaped)
            .field("post_cleanup_child_exit", &self.post_cleanup_child_exit)
            .field("post_cleanup_reader_end", &self.post_cleanup_reader_end)
            .field("reader_join", &self.reader_join)
            .field("reader_started", &self.reader_started)
            .finish()
    }
}
impl CleanupEvidence {
    pub(super) fn succeeds(&self) -> bool {
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
pub(super) struct PtyChild {
    pub(super) service: crate::service::ServiceContext,
    pub(super) pty: String,
    pub(super) tab_id: String,
    pub(super) session: Arc<codeterm_pty::pty::manager::PtySession>,
    pub(super) generation: u64,
    pub(super) reader_evidence: Arc<Mutex<ReaderEvidence>>,
    wake: mpsc::Receiver<()>,
    wake_sender: Option<mpsc::SyncSender<()>>,
    reader_join: ReaderJoinState,
    pub(super) reader_started: bool,
    completion_deadline: Option<Instant>,
    pub(super) completion: Option<CompletionEvidence>,
    cleanup_started: bool,
}
impl PtyChild {
    pub(super) fn new(
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
            reader_join: ReaderJoinState::default(),
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
                let mut frame = Vec::new();
                let mut stages =
                    [ModalStage::Initial, ModalStage::Focus, ModalStage::Closed].into_iter();
                let mut expected = stages.next();
                let end = loop {
                    let mut bytes = [0u8; 8192];
                    match reader.read(&mut bytes) {
                        Ok(0) => break ReaderEnd::Eof,
                        Ok(count) => {
                            let Some(stage) = expected else { continue };
                            frame.extend_from_slice(&bytes[..count]);
                            evidence.lock().record_pending_frame(&frame);
                            if !frame
                                .windows(stage.marker().len())
                                .any(|bytes| bytes == stage.marker().as_bytes())
                            {
                                if frame.len() > MAX_FIXTURE_DIAGNOSTIC_BYTES {
                                    break ReaderEnd::ReadError {
                                        kind: "frame_bound".into(),
                                        raw_os_error: None,
                                    };
                                }
                                continue;
                            }
                            expected = stages.next();
                            service.feed_pty_output_for_generation(&pty, generation, &frame);
                            frame.clear();
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
        self.reader_join = ReaderJoinState::from_handle(reader_join);
        Ok(())
    }
    fn wait_for_screen(&self, stage: ModalStage) -> Result<StageEvent, StageFailure> {
        wait_for_screen(&self.reader_evidence, &self.wake, stage)
    }
    pub(super) fn completion_deadline(&mut self) -> Instant {
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
        self.reader_join.settle(allowed, deadline)
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
        let reader_finished = { self.reader_evidence.lock().reader_finished };
        let reader_join = self.join_reader(
            reader_finished || reader.status == ReaderWaitStatus::NotificationDisconnected,
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
    pub(super) fn cleanup(&mut self, deadline: Instant) -> CleanupEvidence {
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
pub(super) fn run_modal(child: &mut PtyChild, command: &str) -> Result<(), String> {
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
pub(super) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
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
pub(super) fn child_exit_is_success(exit: Option<Option<i32>>) -> bool {
    exit == Some(Some(0))
}
