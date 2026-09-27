use super::*;
use crate::agent::chat_types::ChatMsg;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::{
    AgentProvider, HookEventBinding, HookIngestionRoute, ProviderStateVerdict,
};
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::test_fixture::ServiceFixture;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

const STARTER: &str = "opaque startup token 8871";

struct LaunchEvidenceProvider {
    root: PathBuf,
    wait_for_completion: bool,
}

impl AgentProvider for LaunchEvidenceProvider {
    fn id(&self) -> &str {
        "launch-evidence"
    }

    fn display_name(&self) -> &str {
        "Launch evidence"
    }

    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
        Some(self.root.join(format!("{session_id}.jsonl")))
    }

    fn public_send_waits_for_startup_turn_completion(&self) -> bool {
        self.wait_for_completion
    }

    fn hook_event_roster(&self) -> Vec<HookEventBinding> {
        self.wait_for_completion
            .then(|| HookEventBinding {
                event: "session.idle".into(),
                route: HookIngestionRoute::SessionState,
                verdict: Some(ProviderStateVerdict::Idle),
            })
            .into_iter()
            .collect()
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn session_jsonl_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
        self.session_file_path("", session_id)
    }

    fn parse_session_delta(&self, path: &Path, from_offset: u64) -> SessionDelta {
        let Ok(mut file) = std::fs::File::open(path) else {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        };
        if file.seek(SeekFrom::Start(from_offset)).is_err() {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        }
        let messages = bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                Some(RichMessage {
                    role: value.get("role")?.as_str()?.to_string(),
                    blocks: vec![MessageBlock {
                        kind: "text".into(),
                        data: serde_json::json!({
                            "text": value.get("content")?.as_str()?
                        }),
                    }],
                    ..Default::default()
                })
            })
            .collect();
        SessionDelta {
            new_offset: from_offset + bytes.len() as u64,
            messages,
        }
    }
}

fn launch_fixture() -> (ServiceFixture, String, LaunchAttempt) {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("readiness-test-provider".into());
    tab.pty_id = Some("launch-pty".into());
    tab.agent_session_id = Some("launch-session".into());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.reported_session_binding_nonce = Some("launch-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
    };
    (ServiceFixture::new(mux), tab_id, attempt)
}

fn arm(readiness: &ReadinessRegistry, tab_id: &str, attempt: LaunchAttempt) {
    let key = readiness.arm(tab_id.to_string(), Some(STARTER.into()), None, false);
    readiness.bind_attempt(attempt, key);
    assert!(readiness.bound(tab_id, "launch-nonce", "launch-session"));
}

#[test]
fn public_send_between_launch_and_readiness_becomes_one_turn_after_bootstrap() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = Arc::new(ReadinessRegistry::default());
    let key = readiness.arm(tab_id.clone(), Some(STARTER.into()), None, false);
    let events = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let wait_readiness = readiness.clone();
    let wait_mux = fixture.service.mux.clone();
    let wait_tab = tab_id.clone();
    let wait_events = events.clone();
    let send = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let ready = wait_readiness.await_ready(&wait_mux, &wait_tab, Duration::from_secs(2));
        if ready.is_ok() {
            wait_events.lock().push("brief");
        }
        ready
    });

    started_rx.recv().unwrap();
    assert!(events.lock().is_empty());
    readiness.bind_attempt(attempt, key);
    assert!(readiness.bound(&tab_id, "launch-nonce", "launch-session"));
    events.lock().push("bootstrap");
    let turn = [ChatMsg {
        id: "startup-turn".into(),
        kind: "user".into(),
        content: STARTER.into(),
        ..Default::default()
    }];
    assert!(readiness.note_turn(
        &fixture.service.mux,
        &tab_id,
        "launch-session",
        "launch-nonce",
        &turn,
    ));

    assert!(send.join().unwrap().is_ok());
    assert_eq!(events.lock().as_slice(), &["bootstrap", "brief"]);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("cleared-session".into());
        tab.session_binding_nonce = Some("cleared-nonce".into());
        tab.reported_session_binding_nonce = Some("cleared-nonce".into());
    }
    assert!(readiness
        .inspect(&fixture.service.mux, &tab_id)
        .unwrap()
        .is_ok());
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn public_send_before_readiness_never_reports_success_without_a_turn() {
    let (fixture, tab_id, _attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    readiness.arm(tab_id.clone(), Some(STARTER.into()), None, false);
    let mut payload_writes = 0;

    let result = readiness.await_ready(&fixture.service.mux, &tab_id, Duration::ZERO);
    if result.is_ok() {
        payload_writes += 1;
    }

    assert!(result.is_err());
    assert_eq!(payload_writes, 0);
    assert!(readiness.state.lock().gates.contains_key(&tab_id));
    readiness.retire(&tab_id);
    assert!(readiness
        .inspect(&fixture.service.mux, &tab_id)
        .unwrap()
        .is_ok());
}

#[test]
fn public_send_to_an_already_running_tab_has_no_wait() {
    let (fixture, tab_id, _) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn ordinary_unauthenticated_tab_has_no_public_send_gate() {
    let (fixture, tab_id, _) = launch_fixture();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.session_binding_nonce = None;
        tab.reported_session_binding_nonce = None;
    }
    let readiness = ReadinessRegistry::default();
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn launch_without_startup_turn_retires_previous_gate() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    arm(&readiness, &tab_id, attempt);
    readiness.arm(tab_id.clone(), None, None, false);
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn old_launch_guard_cannot_retire_new_launch_gate() {
    let (fixture, tab_id, _) = launch_fixture();
    let old_key = fixture
        .service
        .arm_public_send_launch(&tab_id, Some("old starter turn".into()), None, false)
        .unwrap();
    let old_guard = fixture
        .service
        .public_send_launch_guard_for_key(&tab_id, Some(old_key));
    let new_key = fixture
        .service
        .arm_public_send_launch(&tab_id, Some("new starter turn".into()), None, false)
        .unwrap();

    drop(old_guard);

    assert!(fixture.service.has_public_send_launch_gate(&tab_id));
    fixture
        .service
        .retire_public_send_launch_key(&tab_id, new_key);
    assert!(!fixture.service.has_public_send_launch_gate(&tab_id));
}

#[test]
fn nonmatching_user_turn_does_not_satisfy_armed_gate() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    arm(&readiness, &tab_id, attempt);
    let unrelated = [ChatMsg {
        id: "other-turn".into(),
        kind: "user".into(),
        content: "a different launch instruction".into(),
        ..Default::default()
    }];
    assert!(!readiness.note_turn(
        &fixture.service.mux,
        &tab_id,
        "launch-session",
        "launch-nonce",
        &unrelated,
    ));
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_err());
}

#[test]
fn failed_launch_retires_armed_gate() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    arm(&readiness, &tab_id, attempt);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .boot_stage = Some(BootStage::SpawnFailed);
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn session_rotation_retires_armed_gate() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let readiness = ReadinessRegistry::default();
    arm(&readiness, &tab_id, attempt);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("rotated-session".into());
        tab.session_binding_nonce = Some("rotated-nonce".into());
        tab.reported_session_binding_nonce = Some("rotated-nonce".into());
    }
    readiness.retire_changed_session(&fixture.service.mux, &tab_id, "rotated-nonce");
    assert!(readiness
        .await_ready(&fixture.service.mux, &tab_id, Duration::ZERO)
        .is_ok());
}

#[test]
fn headless_launch_confirms_from_bound_session_evidence() {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("launch-evidence".into());
    tab.pty_id = Some("launch-pty".into());
    tab.agent_session_id = Some("launch-session".into());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.reported_session_binding_nonce = Some("launch-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let fixture = ServiceFixture::new(mux);
    let root = fixture.temp.path().to_path_buf();
    std::fs::write(root.join("launch-session.jsonl"), b"").unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some(root.to_string_lossy().into_owned());
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(LaunchEvidenceProvider {
            root: root.clone(),
            wait_for_completion: false,
        }));
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
    };
    let service = fixture.service.clone();
    let key = service.arm_public_send_launch(&tab_id, Some(STARTER.into()), None, false);
    service.bind_public_send_launch_attempt(attempt, key);
    crate::service::public_send_readiness::observe_session_binding(
        &service,
        &tab_id,
        "launch-session",
        "launch-nonce",
    );

    let (started_tx, started_rx) = mpsc::channel();
    let waiting_service = service.clone();
    let waiting_tab = tab_id.clone();
    let send = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        waiting_service.await_public_send_readiness(&waiting_tab, Duration::from_secs(10))
    });
    started_rx.recv().unwrap();
    let session_path = root.join("launch-session.jsonl");
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(session_path)
            .unwrap(),
        "{}",
        serde_json::json!({"role":"user", "content":STARTER})
    )
    .unwrap();
    assert!(send.join().unwrap().is_ok());
}

#[test]
fn opencode_capability_holds_public_send_until_startup_turn_idle() {
    let (fixture, tab_id, attempt) = launch_fixture();
    let root = fixture.temp.path().to_path_buf();
    let transcript = root.join("launch-session.jsonl");
    std::fs::write(&transcript, b"").unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("launch-evidence".into());
        tab.cwd = Some(root.to_string_lossy().into_owned());
    }
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(LaunchEvidenceProvider {
            root: root.clone(),
            wait_for_completion: true,
        }));
    let service = fixture.service.clone();
    let wait_for_turn_completion = service
        .agent_registry
        .with_provider("launch-evidence", |provider| {
            provider.public_send_waits_for_startup_turn_completion()
        })
        .unwrap();
    let key = service.arm_public_send_launch(
        &tab_id,
        Some(STARTER.into()),
        None,
        wait_for_turn_completion,
    );
    service.bind_public_send_launch_attempt(attempt, key);
    observe_session_binding(&service, &tab_id, "launch-session", "launch-nonce");
    observe_bootstrap_turn_idle(&service, &tab_id, "session.idle", "launch-session");
    assert!(service
        .await_public_send_readiness(&tab_id, Duration::ZERO)
        .is_err());
    assert!(note_bootstrap_turn(
        &service.mux,
        &tab_id,
        "launch-session",
        "launch-nonce",
        &[ChatMsg {
            id: "startup-turn".into(),
            kind: "user".into(),
            content: STARTER.into(),
            ..Default::default()
        }],
    ));
    assert!(service
        .await_public_send_readiness(&tab_id, Duration::ZERO)
        .is_err());

    observe_bootstrap_turn_idle(&service, &tab_id, "session.idle", "launch-session");

    assert!(service
        .await_public_send_readiness(&tab_id, Duration::ZERO)
        .is_ok());
}
