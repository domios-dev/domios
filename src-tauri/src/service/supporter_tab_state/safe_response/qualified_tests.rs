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
use std::time::{Duration, Instant};

#[cfg(test)]
mod pty;
use pty::{
    child_exit_is_success, panic_message, run_modal, stage_failure, CleanupEvidence,
    CompletionEvidence, ModalStage, PtyChild, ReaderEnd, ReaderEvidence, ReaderJoinState,
    ReaderJoinStatus, ReaderWait, ReaderWaitStatus, StageFailureReason,
};

const STAGE_TIMEOUT: Duration = Duration::from_secs(15);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_STAGE_SCREEN_BYTES: usize = 8 * 1024;
const MAX_MODEL_SCREENS: usize = 4;
const MAX_MODEL_SCREEN_BYTES: usize = 8 * 1024;
const MAX_FIXTURE_DIAGNOSTIC_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_BYTES: usize = 512 * 1024;
const INITIAL_MARKER: &str = "SAFE_TEST_INITIAL_MODAL";
const FOCUS_MARKER: &str = "SAFE_TEST_FOCUS_EVENT";
const CLOSED_MARKER: &str = "SAFE_TEST_MODAL_CLOSED";
const SESSION_ID: &str = "fixture-session-private";
const SESSION_NONCE: &str = "fixture-nonce-private";
const JOURNAL_FIELDS: &str = "phase|observation_id|decision_point|observation_status|decided_by|error_category|reason|classification|model_verdict|alternate_verdict|candidate_evidence|correlation";
const CORRELATION_FIELDS: &str = "version|decision_point|observation_id|launch_id|session_id|occurrence_id|producer|parent_observation_id|frame_hash|candidate_evidence";

struct StaticModalModel {
    model_id: String,
    screens: Arc<Mutex<CapturedScreens>>,
}
#[derive(Clone, Default)]
struct CapturedScreens {
    count: usize,
    snapshots: Vec<BoundedText>,
    question_sets: Vec<Vec<String>>,
    truncated: bool,
    adjacent_equal: bool,
    previous_screen: Option<String>,
    initial_stage_observed: bool,
}
impl CapturedScreens {
    fn record(&mut self, screen: &str) {
        self.count += 1;
        self.initial_stage_observed |= screen.contains(INITIAL_MARKER);
        self.adjacent_equal |= self.previous_screen.as_deref() == Some(screen);
        self.previous_screen = Some(screen.to_string());
        if self.snapshots.len() == MAX_MODEL_SCREENS {
            self.truncated = true;
        } else {
            let safe_screen = super::super::privacy::screen(screen);
            let snapshot = BoundedText::tail(&safe_screen, MAX_MODEL_SCREEN_BYTES);
            self.truncated |= snapshot.truncated;
            self.snapshots.push(snapshot);
        }
    }
    fn has_model_capture(&self) -> bool {
        self.count > 0 && !self.snapshots.is_empty()
    }
}
impl std::fmt::Debug for CapturedScreens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedScreens")
            .field("count", &self.count)
            .field("initial_stage_observed", &self.initial_stage_observed)
            .field("snapshots", &self.snapshots)
            .field("question_sets", &self.question_sets)
            .field("truncated", &self.truncated)
            .field("adjacent_equal", &self.adjacent_equal)
            .finish()
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
        let screen = request.state["screen"].as_str().unwrap_or_default();
        eprintln!(
            "qualified fixture model stage: initial={} focus={} closed={}",
            screen.contains(INITIAL_MARKER),
            screen.contains(FOCUS_MARKER),
            screen.contains(CLOSED_MARKER)
        );
        let mut screens = self.screens.lock();
        screens.record(screen);
        let question_set = request.questions.keys().cloned().collect::<Vec<_>>();
        if !screens.question_sets.contains(&question_set) {
            screens.question_sets.push(question_set);
        }
        drop(screens);
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
    if rows.iter().any(|row| {
        row["phase"] == "owned_model"
            && row["decided_by"] == "adapter"
            && row["reason"] == "declared_choice"
    }) {
        Vec::new()
    } else {
        vec!["owned_model_declared_choice".into()]
    }
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
    identity: Option<IdentityMatches>,
    manager_generation_matches: bool,
    child_exit_before_cleanup: Option<Option<i32>>,
    journal: JournalEvidence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IdentityMatches {
    prompt_cleared: bool,
    session_matches: bool,
    nonce_matches: bool,
    pty_matches: bool,
}
impl IdentityMatches {
    fn all_match(self) -> bool {
        self.prompt_cleared && self.session_matches && self.nonce_matches && self.pty_matches
    }
}
fn capture_modal_evidence(
    child: &PtyChild,
    screens: &Arc<Mutex<CapturedScreens>>,
    journal_path: &Path,
) -> ModalEvidence {
    let reader = child.reader_evidence.lock().clone();
    let identity = child
        .service
        .mux
        .read()
        .get_tab(&child.tab_id)
        .map(|tab| IdentityMatches {
            prompt_cleared: tab.pending_prompt.is_none(),
            session_matches: tab.agent_session_id.as_deref() == Some(SESSION_ID),
            nonce_matches: tab.session_binding_nonce.as_deref() == Some(SESSION_NONCE)
                && tab.reported_session_binding_nonce.as_deref() == Some(SESSION_NONCE),
            pty_matches: tab.pty_id.as_deref() == Some(child.pty.as_str()),
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
    let model_screen_captured = captures.has_model_capture();
    let current_identity = evidence.identity.is_some_and(IdentityMatches::all_match)
        && evidence.manager_generation_matches;
    let completion_matches = completion.is_some_and(CompletionEvidence::succeeds)
        && completion.is_some_and(|c| c.child_exit == evidence.child_exit_before_cleanup);
    reject_unless!(
        evidence.reader_started
            && reader.reader_finished
            && current_identity
            && model_screen_captured
            && captures.initial_stage_observed
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
    let answer = rows
        .iter()
        .find(|row| {
            row["phase"] == "owned_model"
                && row["decided_by"] == "adapter"
                && row["reason"] == "declared_choice"
        })
        .ok_or_else(|| "declared-choice model answer row is absent".to_string())?;
    if answer["observation_status"] != "completed"
        || answer["decision_point"] != "prompt_candidate"
        || answer["model_verdict"].is_null()
        || answer["correlation"]["observation_id"] != answer["observation_id"]
    {
        return Err(format!(
            "provider-declared model answer evidence mismatch: answer={answer:?}"
        ));
    }
    Ok(())
}
fn bounded_diagnostic(text: &str) -> BoundedText {
    let safe_text = super::super::privacy::screen(text);
    BoundedText::tail(&safe_text, MAX_FIXTURE_DIAGNOSTIC_BYTES)
}
#[test]
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
    let _qualification = override_qualification_for_test(
        &adapter,
        &model_id,
        &criteria_version(),
        serde_json::json!({"fixture_authorized": true}),
    );
    assert!(qualified_for(
        &adapter,
        Some(&model_id),
        &criteria_version(),
        "",
        LOGIN_OR_AUTH_REQUIRED
    ));
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
    let initial = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    let focused = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-not-now.txt");
    std::fs::write(fixture.temp.path().join("initial.txt"), initial).unwrap();
    std::fs::write(fixture.temp.path().join("focused.txt"), focused).unwrap();
    let script = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n\
[Console]::Write([char]27 + '[2J' + [char]27 + '[H')\n\
[Console]::Write([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'initial.txt'), [System.Text.Encoding]::UTF8))\n\
[Console]::Write([char]27 + ']7;' + [Uri]::new($PSScriptRoot).AbsoluteUri + [char]7)\n\
[Console]::WriteLine('SAFE_TEST_INITIAL_MODAL')\n\
$first = [Console]::ReadKey($true)\n\
[Console]::Write([char]27 + '[2J' + [char]27 + '[H')\n\
[Console]::Write([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'focused.txt'), [System.Text.Encoding]::UTF8))\n\
[Console]::WriteLine('SAFE_TEST_FOCUS_EVENT')\n\
$second = [Console]::ReadKey($true)\n\
if (($first.KeyChar -eq 'j') -and ($second.Key -eq [System.ConsoleKey]::Enter)) { [Console]::WriteLine('SAFE_TEST_OPTION_SELECTED'); [Console]::WriteLine('SAFE_TEST_MODAL_CLOSED'); exit 0 }\n\
[Console]::WriteLine('SAFE_TEST_WRONG_RESPONSE'); exit 9\n";
    let script_path = fixture.temp.path().join("modal.ps1");
    std::fs::write(&script_path, script).unwrap();
    let command_path = script_path.to_string_lossy().replace('\'', "''");
    let command = format!(
        "Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; $global:LASTEXITCODE = 98; $ErrorActionPreference = 'Stop'; try {{ & '{command_path}'; $scriptExit = $global:LASTEXITCODE; if ($scriptExit -eq 98) {{ throw 'fixture script returned without an exit code' }}; [Console]::Out.Flush(); [Console]::Error.Flush(); [System.Environment]::Exit([int]$scriptExit) }} catch {{ try {{ [Console]::Error.WriteLine('SAFE_TEST_SETUP_FAILURE: {{0}}', $_.Exception.Message); [Console]::Error.Flush() }} finally {{ [System.Environment]::Exit(98) }} }}\r"
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
    fixture.service.screen_state.lock().resize(&pty, 120, 40);
    let mut child = PtyChild::new(fixture.service.clone(), pty, tab_id.clone(), session);
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
    let completion = child.completion.clone();
    let journal = std::fs::read_to_string(&journal_path).unwrap_or_default();
    let safe_response_rows = journal
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|row| {
            row["action"] == "owned_prompt_answer" || row["decision_point"] == "safe_response"
        })
        .map(|row| {
            let asked_names = row["asked"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|answer| answer["name"].as_str())
                .collect::<Vec<_>>();
            serde_json::json!({
                "action": row["action"],
                "phase": row["phase"],
                "reason": row["reason"],
                "decision_point": row["decision_point"],
                "observation_id": row["observation_id"],
                "observation_status": row["observation_status"],
                "decided_by": row["decided_by"],
                "asked_names": asked_names,
            })
        })
        .collect::<Vec<_>>();
    let current_identity = evidence.identity.is_some_and(IdentityMatches::all_match)
        && evidence.manager_generation_matches;
    let model_screen_captured = evidence.captured_screens.has_model_capture();
    let agreement_observations = AGREEMENT
        .lock()
        .get(&tab_id)
        .map(|agreement| agreement.count);
    let pending_prompt = fixture.service.mux.read().get_tab(&tab_id).map(|tab| {
        serde_json::json!({
            "present": tab.pending_prompt.is_some(),
            "kind": tab.pending_prompt_kind.map(|kind| format!("{kind:?}")),
            "options": tab.pending_prompt.as_ref().map(|prompt| prompt.options.len()),
            "session_authenticated": tab.launch_session_authenticated(),
        })
    });
    let completion_matches = completion
        .as_ref()
        .is_some_and(CompletionEvidence::succeeds)
        && completion
            .as_ref()
            .is_some_and(|c| c.child_exit == evidence.child_exit_before_cleanup);
    eprintln!(
        "qualified fixture validation facts: reader_started={} reader_finished={} model_screen_captured={} current_identity={} agreement_observations={:?} pending_prompt={:?} completion_matches={}",
        evidence.reader_started,
        evidence.reader.reader_finished,
        model_screen_captured,
        current_identity,
        agreement_observations,
        pending_prompt,
        completion_matches
    );
    eprintln!(
        "qualified fixture safe-response activity: {:?}",
        bounded_diagnostic(&format!("{safe_response_rows:?}"))
    );
    let identity = evidence.identity.unwrap_or(IdentityMatches {
        prompt_cleared: false,
        session_matches: false,
        nonce_matches: false,
        pty_matches: false,
    });
    eprintln!(
        "qualified fixture primary: {:?}",
        bounded_diagnostic(&format!("{primary:?}"))
    );
    eprintln!(
        "qualified fixture pre-cleanup identity matches: tab_present={} prompt_cleared={} session_matches={} nonce_matches={} pty_matches={} manager_generation_matches={}",
        evidence.identity.is_some(),
        identity.prompt_cleared,
        identity.session_matches,
        identity.nonce_matches,
        identity.pty_matches,
        evidence.manager_generation_matches
    );
    eprintln!(
        "qualified fixture model capture summary: calls={} diagnostic_snapshots={} question_sets={:?} adjacent_equal={} truncated={}",
        evidence.captured_screens.count,
        evidence.captured_screens.snapshots.len(),
        evidence.captured_screens.question_sets,
        evidence.captured_screens.adjacent_equal,
        evidence.captured_screens.truncated
    );
    eprintln!(
        "qualified fixture pre-cleanup evidence: {:?}",
        bounded_diagnostic(&format!("{evidence:#?}"))
    );
    eprintln!(
        "qualified fixture validation: {:?}",
        bounded_diagnostic(&format!("{validation:?}"))
    );
    eprintln!(
        "qualified fixture completion receipt: {:?}",
        bounded_diagnostic(&format!("{completion:#?}"))
    );
    let completion_deadline = child.completion_deadline();
    let cleanup = child.cleanup(completion_deadline);
    eprintln!(
        "qualified fixture cleanup receipt: {:?}",
        bounded_diagnostic(&format!("{cleanup:#?}"))
    );
    assert!(
        primary.is_ok() && validation.as_ref().is_some_and(Result::is_ok) && cleanup.succeeds(),
        "qualified fixture primary, validation, or cleanup fact failed; see bounded receipts above"
    );
}
#[test]
fn qualified_fixture_capture_summary_survives_diagnostic_truncation() {
    let mut captures = CapturedScreens::default();
    for screen in ["one", "two", "three", "four", "four"] {
        captures.record(screen);
    }
    assert_eq!(captures.count, 5);
    assert_eq!(captures.snapshots.len(), MAX_MODEL_SCREENS);
    assert!(captures.truncated);
    assert!(captures.adjacent_equal);
    assert!(captures.has_model_capture());
}
#[test]
fn qualified_fixture_stage_timeout_retains_actual_last_snapshot() {
    let mut evidence = ReaderEvidence::new();
    evidence.record_screen("fixture output without a stage marker");
    let pending = "opaque startup output".repeat(MAX_STAGE_SCREEN_BYTES);
    evidence.record_pending_frame(pending.as_bytes());
    assert_eq!(evidence.sequence, 1);
    assert!(evidence.stage_events.is_empty());
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
    let diagnostic = failure.pending_output.unwrap();
    assert!(diagnostic.truncated);
    assert!(diagnostic.text.len() <= MAX_STAGE_SCREEN_BYTES);
    let tail = diagnostic.text.strip_prefix("[truncated]\n").unwrap();
    assert!(pending.ends_with(tail));
    evidence.record_screen("completed frame");
    assert!(evidence.pending_output.is_none());
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

    let deadline = Instant::now() + COMPLETION_TIMEOUT;
    let cleanup_receipt = |reader_join| CleanupEvidence {
        generation_matched: true,
        begin_kill_error: None,
        reap_error: None,
        reaped: true,
        post_cleanup_child_exit: Some(Some(0)),
        post_cleanup_reader_end: Some(ReaderEnd::Eof),
        reader_join,
        reader_started: true,
    };
    let mut normal_join = ReaderJoinState::from_handle(std::thread::spawn(|| {}));
    let completion_join = normal_join.settle(true, deadline);
    let cleanup_join = normal_join.settle(true, deadline);
    assert_eq!(completion_join, ReaderJoinStatus::Joined);
    assert_eq!(cleanup_join, completion_join);
    let successful_completion = CompletionEvidence {
        child_exit: Some(Some(0)),
        child_exit_observed_ms: Some(0),
        reader: ReaderWait {
            end: Some(ReaderEnd::Eof),
            status: ReaderWaitStatus::Completed,
            observed_elapsed_ms: Some(0),
        },
        reader_join: completion_join,
        within_deadline: true,
        elapsed_ms: 0,
    };
    assert!(successful_completion.succeeds());
    assert!(cleanup_receipt(cleanup_join).succeeds());

    let mut never_joined = ReaderJoinState::default();
    let never_joined = never_joined.settle(true, deadline);
    assert_eq!(never_joined, ReaderJoinStatus::NotAttempted);
    assert!(!cleanup_receipt(never_joined).succeeds());

    let mut panicked_join = ReaderJoinState::from_handle(std::thread::spawn(|| {
        panic!("fixture reader panic");
    }));
    let panicked_join = panicked_join.settle(true, deadline);
    assert!(matches!(&panicked_join, ReaderJoinStatus::Panicked(_)));
    assert!(!cleanup_receipt(panicked_join).succeeds());

    let mut expired_state = ReaderJoinState::from_handle(std::thread::spawn(|| {}));
    let expired_deadline = Instant::now() - Duration::from_secs(1);
    let expired_join = expired_state.settle(true, expired_deadline);
    assert_eq!(expired_join, ReaderJoinStatus::Deadline);
    assert_eq!(expired_join, expired_state.settle(true, deadline));
    assert!(!cleanup_receipt(expired_join).succeeds());
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
