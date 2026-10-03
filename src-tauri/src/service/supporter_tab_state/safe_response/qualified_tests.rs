use super::*;
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionMetadata, DecisionModel, DecisionModelInfo,
    DecisionPrimitives, DecisionQuestion, DecisionQuestionsRequest, DecisionResponse, TokenMode,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct StaticModalModel {
    model_id: String,
    screens: Arc<Mutex<Vec<String>>>,
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

    fn metadata(&self) -> DecisionMetadata {
        DecisionMetadata::default()
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
        self.screens.lock().push(
            request.state["screen"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
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

struct PtyChild {
    service: crate::service::ServiceContext,
    pty: String,
    reader_finished: mpsc::Receiver<()>,
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        let _ = self.service.pty.lock().kill(&self.pty);
    }
}

fn wait_for_screen(rx: &mpsc::Receiver<String>, marker: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "PTY child omitted event marker {marker}"
        );
        let screen = rx
            .recv_timeout(remaining)
            .expect("PTY output event did not arrive before the failure deadline");
        if screen.contains(marker) {
            return screen;
        }
    }
}

fn test_qualification() -> serde_json::Value {
    serde_json::json!({ "fixture_authorized": true })
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
        tab.agent_session_id = Some("fixture-session-private".into());
        tab.session_binding_nonce = Some("fixture-nonce-private".into());
        tab.reported_session_binding_nonce = Some("fixture-nonce-private".into());
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
    let screens = Arc::new(Mutex::new(Vec::new()));
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
        test_qualification(),
    );

    fixture
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
    let session = fixture.service.pty.lock().get_session(&pty).unwrap();
    let generation = session.generation;
    assert!(fixture
        .service
        .screen_state
        .lock()
        .bind_pty_generation(&pty, generation));
    let mut reader = session.reader.lock().unwrap().take().unwrap();
    let service = fixture.service.clone();
    let reader_pty = pty.clone();
    let (screen_tx, screen_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = [0u8; 8192];
        loop {
            let count = match reader.read(&mut bytes) {
                Ok(count) if count > 0 => count,
                _ => break,
            };
            service.feed_pty_output_for_generation(&reader_pty, generation, &bytes[..count]);
            let screen = service
                .screen_state
                .lock()
                .get_screen_text(&reader_pty)
                .unwrap_or_default();
            if screen_tx.send(screen).is_err() {
                break;
            }
        }
        let _ = done_tx.send(());
    });
    let child = PtyChild {
        service: fixture.service.clone(),
        pty: pty.clone(),
        reader_finished: done_rx,
    };

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
    fixture
        .service
        .pty
        .lock()
        .write_and_flush(
            &pty,
            format!("& '{command_path}'\r").as_bytes(),
            Duration::from_secs(2),
        )
        .expect("start the modal scenario before it waits for provider keys");

    wait_for_screen(&screen_rx, "SAFE_TEST_INITIAL_MODAL");
    wait_for_screen(&screen_rx, "SAFE_TEST_FOCUS_EVENT");
    let closed = wait_for_screen(&screen_rx, "SAFE_TEST_MODAL_CLOSED");
    assert!(!closed.contains("SAFE_TEST_WRONG_RESPONSE"));
    child
        .reader_finished
        .recv_timeout(Duration::from_secs(3))
        .expect("PTY child completion must close its output stream");

    let captured_screens = screens.lock().clone();
    assert!(captured_screens.len() >= 4);
    assert!(captured_screens.windows(2).any(|pair| pair[0] == pair[1]));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.pending_prompt.is_none()));
    let journal = std::fs::read_to_string(fixture.temp.path().join("supporter/journal.jsonl"))
        .expect("fixture journal records completed observations");
    assert!(!journal.contains("fixture-session-private"));
    assert!(!journal.contains("fixture-nonce-private"));
    let entries: Vec<serde_json::Value> = journal
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let seed = entries
        .iter()
        .find(|entry| entry["correlation"]["producer"] == "prompt_established")
        .expect("prompt publication starts one post-publication capture");
    let confirmation = entries
        .iter()
        .find(|entry| entry["correlation"]["producer"] == "prompt_confirmation")
        .expect("a confident seed schedules one real confirmation capture");
    assert_eq!(seed["observation_status"], "completed");
    assert_eq!(confirmation["observation_status"], "completed");
    assert_eq!(seed["decision_point"], "prompt_candidate");
    assert_eq!(confirmation["decision_point"], "prompt_candidate");
    assert_eq!(
        seed["correlation"]["occurrence_id"],
        confirmation["correlation"]["occurrence_id"]
    );
    assert_eq!(
        seed["correlation"]["frame_hash"],
        confirmation["correlation"]["frame_hash"]
    );
    assert_ne!(seed["observation_id"], confirmation["observation_id"]);
    assert_eq!(
        confirmation["correlation"]["parent_observation_id"],
        seed["observation_id"]
    );
    let action = entries
        .iter()
        .find(|entry| {
            entry["phase"] == "action_authorized"
                && entry["observation_id"] == confirmation["observation_id"]
        })
        .expect("action authorization retains the confirming capture identity");
    assert_eq!(action["correlation"], confirmation["correlation"]);
    assert_eq!(action["observation_status"], "completed");
}
