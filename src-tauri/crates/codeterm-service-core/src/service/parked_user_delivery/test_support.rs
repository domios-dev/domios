use super::*;
use crate::service::test_fixture::ServiceFixture;
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::Manager;

pub(super) const TEST_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn pre_ready_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.pty_id = Some("opaque-pty".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-current-nonce".into());
    tab.reported_session_binding_nonce = Some("opaque-old-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
    tab.boot_failure_reason = None;
    let fixture = ServiceFixture::new(mux);
    let session: crate::session::SessionState = Arc::new(Mutex::new(
        crate::session::SessionStore::for_test_dir(fixture.temp.path().join("sessions")),
    ));
    assert!(fixture._app.manage(session));
    assert!(fixture
        ._app
        .manage(Arc::new(fixture.service.clone()) as crate::service::ServiceState));
    (fixture, tab_id)
}

pub(crate) type SubmitBoundary =
    Arc<dyn Fn(&UserSendRecord, &str) -> Result<(), DeliveryError> + Send + Sync>;
pub(super) static SUBMIT_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, SubmitBoundary>>,
> = std::sync::OnceLock::new();

pub struct UserSubmitBoundary(String);

impl UserSubmitBoundary {
    pub fn install(tab_id: &str, boundary: SubmitBoundary) -> Self {
        SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for UserSubmitBoundary {
    fn drop(&mut self) {
        SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn submit_boundary(tab_id: &str) -> Option<SubmitBoundary> {
    SUBMIT_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
}

pub(super) type BeforeSubmit = Arc<dyn Fn() + Send + Sync>;
pub(super) static BEFORE_SUBMIT_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, BeforeSubmit>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) struct BeforeUserSubmitBoundary(String);

#[cfg(test)]
impl BeforeUserSubmitBoundary {
    pub fn install(tab_id: &str, before_submit: BeforeSubmit) -> Self {
        BEFORE_SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.into(), before_submit);
        Self(tab_id.into())
    }
}

#[cfg(test)]
impl Drop for BeforeUserSubmitBoundary {
    fn drop(&mut self) {
        BEFORE_SUBMIT_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn run_before_submit(tab_id: &str) {
    if let Some(before_submit) = BEFORE_SUBMIT_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
    {
        before_submit();
    }
}

pub(super) type OwnerIdle = Arc<dyn Fn() + Send + Sync>;
pub(super) static OWNER_IDLE_BOUNDARIES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, OwnerIdle>>,
> = std::sync::OnceLock::new();

pub(super) struct OwnerIdleBoundary(String);

impl OwnerIdleBoundary {
    pub(super) fn install(tab_id: &str, boundary: OwnerIdle) -> Self {
        OWNER_IDLE_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.to_string(), boundary);
        Self(tab_id.to_string())
    }
}

impl Drop for OwnerIdleBoundary {
    fn drop(&mut self) {
        OWNER_IDLE_BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(super) fn notify_owner_idle(tab_id: &str) {
    let boundary = OWNER_IDLE_BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned();
    if let Some(boundary) = boundary {
        boundary();
    }
}

pub(super) struct QueueProvider {
    pub(super) root: Option<std::path::PathBuf>,
    pub(super) paste_limit: Option<usize>,
    pub(super) lazy_session_start: bool,
}

impl crate::agent::provider::AgentProvider for QueueProvider {
    fn id(&self) -> &str {
        "opaque-provider"
    }
    fn display_name(&self) -> &str {
        "Opaque Provider"
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
    fn session_starts_on_first_user_turn(&self) -> bool {
        self.lazy_session_start
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, session: &str) -> Option<std::path::PathBuf> {
        self.root
            .as_ref()
            .map(|root| root.join(format!("{session}.jsonl")))
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> {
        self.session_file_path(cwd, session)
    }
    fn session_exists(&self, _: &str, _: &str) -> Option<bool> {
        Some(true)
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn paste_limit_bytes(&self) -> Option<usize> {
        self.paste_limit
    }
    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).unwrap();
        file.seek(SeekFrom::Start(from_offset)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let messages = bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_slice(line).ok()?;
                Some(crate::agent::chat_types::RichMessage {
                    role: "user".into(),
                    blocks: vec![crate::agent::chat_types::MessageBlock {
                        kind: "text".into(),
                        data: serde_json::json!({ "text": value["content"].as_str()? }),
                    }],
                    ..Default::default()
                })
            })
            .collect();
        crate::agent::chat_types::SessionDelta {
            new_offset: from_offset + bytes.len() as u64,
            messages,
        }
    }
}

pub fn queue_fixture(anchor: bool, paste_limit: Option<usize>) -> (ServiceFixture, String) {
    queue_fixture_with_session_mode(anchor, paste_limit, false)
}

pub(super) fn queue_fixture_with_session_mode(
    anchor: bool,
    paste_limit: Option<usize>,
    lazy_session_start: bool,
) -> (ServiceFixture, String) {
    let (fixture, tab) = pre_ready_fixture();
    let root = fixture.temp.path().join("transcripts");
    std::fs::create_dir(&root).unwrap();
    if anchor {
        std::fs::write(root.join("opaque-session.jsonl"), b"").unwrap();
    }
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(QueueProvider {
            root: Some(root),
            paste_limit,
            lazy_session_start,
        }));
    (fixture, tab)
}

pub fn await_owner_idle(fixture: &ServiceFixture, tab_id: &str) {
    fixture
        .service
        .ready_user_send_tasks
        .wait(tab_id, TEST_DEADLINE);
    let owner = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(tab_id);
    let (idle_tx, idle_rx) = mpsc::sync_channel(1);
    let callback_tx = idle_tx.clone();
    let _boundary = OwnerIdleBoundary::install(
        tab_id,
        Arc::new(move || {
            let _ = callback_tx.try_send(());
        }),
    );
    fixture
        .service
        .wake_user_owner(tab_id, super::UserWake::Housekeeping);
    loop {
        idle_rx
            .recv_timeout(TEST_DEADLINE)
            .expect("user owner reaches its idle boundary");
        if !owner.running.load(Ordering::Acquire) {
            fixture
                .service
                .ready_user_send_tasks
                .wait(tab_id, TEST_DEADLINE);
            return;
        }
    }
}

pub fn attach_fixture_pty(fixture: &ServiceFixture, tab_id: &str) -> u64 {
    let pty_id = fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    if let Some(session) = fixture.service.pty.lock().get_session(&pty_id) {
        return session.generation;
    }
    let (_, session) = {
        let gate = fixture.service.pty.lock().spawn_gate();
        crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default()).map(
            |built| {
                fixture
                    .service
                    .pty
                    .lock()
                    .register_session(Some(pty_id.clone()), built)
            },
        )
    }
    .expect("spawn the isolated ServiceContext PTY fixture");
    let generation = session.generation;
    assert!(fixture
        .service
        .screen_state
        .lock()
        .bind_pty_generation(&pty_id, generation));
    generation
}
