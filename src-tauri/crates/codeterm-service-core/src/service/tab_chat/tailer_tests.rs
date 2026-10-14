//! Live integration: spawn a tailer over a real JSONL file that
//! grows during the test. Uses the actual claude JS plugin (no
//! mocks) so the JS↔Rust delta path is exercised end-to-end.
//!
//! # Why synthetic plugin instead of HOME env override
//!
//! On Windows, `dirs::home_dir()` uses the Windows Shell `SHGetKnownFolderPath`
//! API, which ignores `USERPROFILE` env-var mutations at runtime. To keep the
//! test hermetic on all platforms we build a synthetic claude plugin whose
//! manifest's `sessions.rootDir` is set to an absolute tempdir — then
//! `host.expandHome(absolutePath)` returns the path unchanged (the `~/`
//! prefix check fails, non-tilde paths pass through). No HOME redirect needed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tempfile::TempDir;

use crate::agent::chat_types::RichMessage;
use crate::agent::registry::AgentRegistry;
use crate::service::tab_chat::store_watch::{spawn_store_watch_with_test_hook, StoreWatchTestHook};
use crate::service::tab_chat::{
    spawn_store_watch, spawn_tailer, ChatRegistry, DeltaSink, StoreWatchConfig, TailConfig,
};

struct Collector {
    msgs: Mutex<Vec<RichMessage>>,
}
impl DeltaSink for Collector {
    fn emit(&self, _tab_id: &str, messages: Vec<RichMessage>, _start_offset: u64, _offset: u64) {
        self.msgs.lock().extend(messages);
    }
}

fn wait_until<F: Fn() -> bool>(deadline_ms: u64, f: F) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(deadline_ms) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn workspace_plugins_root() -> PathBuf {
    crate::test_support::fixture_manifest_dir()
        .join("..")
        .join("plugins")
}

/// Build a registry with a synthetic claude plugin whose sessions.rootDir
/// is `sessions_root_dir`. That way `host.expandHome(rootDir)` returns the
/// absolute path directly — no HOME/USERPROFILE env fiddling required.
fn registry_with_claude_at(sessions_root_dir: &std::path::Path) -> Arc<AgentRegistry> {
    let plugin_js = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-claude")
            .join("plugin.js"),
    )
    .expect("read claude plugin.js");

    // Forward-slash the path so JSON doesn't choke on Windows backslashes.
    let root_dir_fwd = sessions_root_dir.to_string_lossy().replace('\\', "/");

    let manifest_json = format!(
        r#"{{
            "id": "claude",
            "version": "0.1.0",
            "displayName": "Claude Code",
            "spawn": {{
                "readinessStrategy": {{"kind": "fixed_delay", "durationMs": 0}},
                "taskDelivery": "pty_input",
                "verifySubmit": {{"kind": "none"}}
            }},
            "sessions": {{
                "rootDir": "{root_dir_fwd}",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            }}
        }}"#
    );

    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(&manifest_json).expect("parse synthetic claude manifest");

    let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
        "claude",
        plugin_js,
        Some(manifest_json),
    )
    .expect("init claude VM for tailer test");

    let provider = crate::plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm));

    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(provider));
    Arc::new(reg)
}

struct DelayedStoreProvider {
    path: PathBuf,
    available: Arc<AtomicBool>,
}

impl crate::agent::provider::AgentProvider for DelayedStoreProvider {
    fn id(&self) -> &str {
        "delayed-store"
    }
    fn display_name(&self) -> &str {
        "Delayed Store"
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn session_jsonl_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        self.available
            .load(Ordering::Acquire)
            .then(|| self.path.clone())
    }
    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        crate::agent::chat_types::SessionDelta {
            new_offset: std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(from_offset),
            messages: vec![RichMessage {
                role: "assistant".into(),
                ..Default::default()
            }],
        }
    }
}

/// Deterministically resolves `path` on every poll regardless of whether the
/// file exists on disk — mirrors real providers (e.g. claude) whose
/// `session_jsonl_path` is a pure function of cwd/session_id, so a deleted
/// file is a "disappeared" case, not a "path unknown" one.
struct AlwaysPathProvider {
    path: PathBuf,
}

impl crate::agent::provider::AgentProvider for AlwaysPathProvider {
    fn id(&self) -> &str {
        "always-path"
    }
    fn display_name(&self) -> &str {
        "Always Path"
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn session_jsonl_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        Some(self.path.clone())
    }
    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::chat_types::SessionDelta {
        crate::agent::chat_types::SessionDelta {
            new_offset: std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(from_offset),
            messages: vec![RichMessage {
                role: "assistant".into(),
                ..Default::default()
            }],
        }
    }
}

struct SignalledCollector {
    ready: std::sync::mpsc::SyncSender<()>,
}

impl DeltaSink for SignalledCollector {
    fn emit(&self, _: &str, _: Vec<RichMessage>, _: u64, _: u64) {
        let _ = self.ready.try_send(());
    }
}

#[test]
fn event_tailer_keeps_the_session_across_file_replacement() {
    let tmp = TempDir::new().unwrap();
    let jsonl = tmp.path().join("own.jsonl");
    std::fs::write(&jsonl, b"initial\n").unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(AlwaysPathProvider {
        path: jsonl.clone(),
    }));
    let (ready, messages) = std::sync::mpsc::sync_channel(8);
    let handle = spawn_tailer(
        TailConfig {
            tab_id: "own-tab".into(),
            provider_id: "always-path".into(),
            cwd: "/proj".into(),
            session_id: "own-session".into(),
            start_offset: 0,
        },
        Arc::new(registry),
        Arc::new(SignalledCollector { ready }),
    );
    messages.recv_timeout(Duration::from_secs(2)).unwrap();
    std::fs::remove_file(&jsonl).unwrap();
    std::fs::write(&jsonl, b"replacement\n").unwrap();
    messages.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(handle.is_alive());
    handle.shutdown();
}

/// A pending store watch has no missing-poll budget at all: a store that
/// flaps unavailable/available for longer than a materialized tailer's own
/// reap window must not cost the watch its eligibility for promotion.
#[test]
fn pending_store_watch_survives_flapping_availability_before_promotion() {
    let tmp = TempDir::new().unwrap();
    let sid = "sess-flap-001";
    let jsonl = tmp.path().join(format!("{sid}.jsonl"));
    let available = Arc::new(AtomicBool::new(false));
    let agent_registry = AgentRegistry::new();
    agent_registry.register_provider(Box::new(DelayedStoreProvider {
        path: jsonl.clone(),
        available: available.clone(),
    }));

    let chat_registry = ChatRegistry::new();
    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let watch = spawn_store_watch(
        StoreWatchConfig {
            tab_id: "p-flap".into(),
            provider_id: "delayed-store".into(),
            cwd: "/project".into(),
            session_id: sid.into(),
            session_binding_nonce: None,
            poll_interval: Duration::from_millis(15),
        },
        Arc::new(agent_registry),
        chat_registry.clone(),
        sink.clone() as Arc<dyn DeltaSink>,
    );
    assert!(watch.register(&chat_registry, "desktop".into()));

    for _ in 0..5 {
        available.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(15));
        available.store(false, Ordering::Release);
        std::thread::sleep(Duration::from_millis(15));
    }

    assert!(
        chat_registry.is_running("p-flap"),
        "a flapping store must not exhaust the pending watch's eligibility for promotion",
    );

    std::fs::write(&jsonl, "materialized\n").unwrap();
    available.store(true, Ordering::Release);

    assert!(
        wait_until(2000, || !sink.msgs.lock().is_empty()),
        "watch must still promote once the store settles",
    );
    chat_registry.stop("p-flap", "desktop");
}

#[test]
fn binding_completion_preserves_new_session_stream() {
    use std::io::Write;

    struct ChannelSink(std::sync::mpsc::Sender<Vec<RichMessage>>);
    impl DeltaSink for ChannelSink {
        fn emit(&self, _: &str, messages: Vec<RichMessage>, _: u64, _: u64) {
            self.0.send(messages).unwrap();
        }
    }

    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("-proj-bind");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("new-session.jsonl");
    std::fs::write(&path, b"").unwrap();
    let providers = registry_with_claude_at(tmp.path());
    let chat = ChatRegistry::new();
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = spawn_tailer(
        TailConfig {
            tab_id: "mounted-chat".into(),
            provider_id: "claude".into(),
            cwd: "/proj/bind".into(),
            session_id: "new-session".into(),
            start_offset: 0,
        },
        providers,
        Arc::new(ChannelSink(sender)),
    );
    assert!(chat.insert(
        "mounted-chat".into(),
        "new-session".into(),
        None,
        "desktop".into(),
        handle,
    ));
    let source = chat.jsonl_source("mounted-chat");

    chat.shutdown_session("mounted-chat", "previous-session");
    assert!(chat.acquire("mounted-chat", "new-session", None, "desktop"));
    assert!(Arc::ptr_eq(&source, &chat.jsonl_source("mounted-chat")));

    for answer in ["first-answer", "later-answer"] {
        let record = serde_json::json!({
            "type": "assistant",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": answer }] },
        });
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{record}").unwrap();
        file.flush().unwrap();
        drop(file);
        let rows = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].blocks[0].data["text"], answer);
    }
    chat.shutdown_session("mounted-chat", "new-session");
    assert!(!chat.is_running("mounted-chat"));
}

#[test]
fn tailer_streams_new_messages_as_file_grows() {
    use std::io::Write;

    struct ChannelSink(std::sync::mpsc::Sender<Vec<RichMessage>>);
    impl DeltaSink for ChannelSink {
        fn emit(&self, _: &str, messages: Vec<RichMessage>, _: u64, _: u64) {
            self.0.send(messages).unwrap();
        }
    }

    assert!(workspace_plugins_root().join("provider-claude").exists());
    let tmp = TempDir::new().unwrap();
    let cwd = "/proj/tail-demo";
    let sid = "sess-tail-001";
    let proj_dir = tmp.path().join("-proj-tail-demo");
    std::fs::create_dir_all(&proj_dir).unwrap();
    let jsonl = proj_dir.join(format!("{sid}.jsonl"));
    std::fs::write(
        &jsonl,
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    )
    .unwrap();
    let registry = registry_with_claude_at(tmp.path());
    let (sender, receiver) = std::sync::mpsc::channel();
    let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
    let handle = super::tailer::spawn_tailer_with_ready(
        TailConfig {
            tab_id: "p1".into(),
            provider_id: "claude".into(),
            cwd: cwd.into(),
            session_id: sid.into(),
            start_offset: 0,
        },
        registry,
        Arc::new(ChannelSink(sender)),
        ready_sender,
    );
    ready_receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("tailer must resolve its source and install the watcher before timing delivery");
    let initial = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("initial message must arrive within 2s after watcher readiness");
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].role, "user");
    assert_eq!(initial[0].blocks[0].data["text"], "hi");

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&jsonl)
        .unwrap();
    writeln!(
        file,
        "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"thanks\"}}}}"
    )
    .unwrap();
    file.flush().unwrap();
    drop(file);
    let appended = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("file growth must deliver a new message within 2s after readiness");
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].role, "assistant");
    assert_eq!(appended[0].blocks[0].data["text"], "thanks");
    handle.shutdown();
}

#[test]
fn tailer_repopulates_when_session_file_appears_late() {
    if !workspace_plugins_root().join("provider-claude").exists() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let cwd = "/proj/late-file";
    let sid = "sess-late-001";
    let proj_dir = tmp.path().join("-proj-late-file");
    std::fs::create_dir_all(&proj_dir).unwrap();
    let jsonl = proj_dir.join(format!("{sid}.jsonl"));
    // The session file does NOT exist yet — claude hasn't completed turn 1.

    let registry = registry_with_claude_at(tmp.path());
    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let handle = spawn_tailer(
        TailConfig {
            tab_id: "p-late".into(),
            provider_id: "claude".into(),
            cwd: cwd.into(),
            session_id: sid.into(),
            start_offset: 0,
        },
        registry,
        sink.clone() as Arc<dyn DeltaSink>,
    );

    // 50ms = 500ms). On the buggy code the tailer thread has already exited.
    std::thread::sleep(Duration::from_millis(900));

    // Now claude finishes turn 1 and the JSONL appears.
    std::fs::write(
        &jsonl,
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    )
    .unwrap();

    assert!(
        wait_until(3000, || sink.msgs.lock().len() >= 1),
        "tailer must keep watching and populate once the session file appears (no permanent give-up)"
    );
    handle.shutdown();
}

#[test]
fn pending_store_watch_promotes_to_tailer_and_streams_initial_batch() {
    let tmp = TempDir::new().unwrap();
    let sid = "sess-pending-watch-001";
    let jsonl = tmp.path().join(format!("{sid}.jsonl"));
    let available = Arc::new(AtomicBool::new(false));
    let agent_registry = AgentRegistry::new();
    agent_registry.register_provider(Box::new(DelayedStoreProvider {
        path: jsonl.clone(),
        available: available.clone(),
    }));

    let chat_registry = ChatRegistry::new();
    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let watch = spawn_store_watch(
        StoreWatchConfig {
            tab_id: "p-pending".into(),
            provider_id: "delayed-store".into(),
            cwd: "/project".into(),
            session_id: sid.into(),
            session_binding_nonce: None,
            poll_interval: Duration::from_millis(20),
        },
        Arc::new(agent_registry),
        chat_registry.clone(),
        sink.clone() as Arc<dyn DeltaSink>,
    );
    assert!(watch.register(&chat_registry, "desktop".into()));

    std::thread::sleep(Duration::from_millis(3100));
    assert!(
        sink.msgs.lock().is_empty(),
        "the missing store emits nothing after more than three seconds"
    );
    assert!(
        chat_registry.is_running("p-pending"),
        "a pending store remains watched until explicit stop, rotation, or promotion",
    );
    std::fs::write(&jsonl, "materialized\n").unwrap();
    available.store(true, Ordering::Release);

    assert!(
        wait_until(2000, || !sink.msgs.lock().is_empty()),
        "one watch must promote to a tailer and emit the initial batch",
    );
    chat_registry.shutdown_tab("p-pending");
    assert!(
        !chat_registry.is_running("p-pending"),
        "one mount is released by one stop after promotion",
    );
}

#[test]
fn pending_store_watch_needs_registration_authority_before_promotion() {
    use std::sync::{mpsc, Barrier};

    let tmp = TempDir::new().unwrap();
    let sid = "sess-registration-race-001";
    let jsonl = tmp.path().join(format!("{sid}.jsonl"));
    std::fs::write(&jsonl, "materialized\n").unwrap();

    let available = Arc::new(AtomicBool::new(true));
    let agent_registry = AgentRegistry::new();
    agent_registry.register_provider(Box::new(DelayedStoreProvider {
        path: jsonl,
        available,
    }));

    let chat_registry = ChatRegistry::new();
    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let (store_seen_tx, store_seen_rx) = mpsc::sync_channel(1);
    let (authority_rejected_tx, authority_rejected_rx) = mpsc::sync_channel(1);
    let resume_after_store_seen = Arc::new(Barrier::new(2));
    let watch = spawn_store_watch_with_test_hook(
        StoreWatchConfig {
            tab_id: "p-registration-race".into(),
            provider_id: "delayed-store".into(),
            cwd: "/project".into(),
            session_id: sid.into(),
            session_binding_nonce: None,
            poll_interval: Duration::from_millis(5),
        },
        Arc::new(agent_registry),
        chat_registry.clone(),
        sink.clone() as Arc<dyn DeltaSink>,
        StoreWatchTestHook {
            store_seen: store_seen_tx,
            resume_after_store_seen: resume_after_store_seen.clone(),
            authority_rejected: authority_rejected_tx,
        },
    );

    store_seen_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("watch must observe the store before registration");
    resume_after_store_seen.wait();
    authority_rejected_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("unregistered watch must decline promotion");

    assert!(watch.register(&chat_registry, "desktop".into()));
    assert!(
        wait_until(2000, || !sink.msgs.lock().is_empty()),
        "registered watch must promote and emit its initial batch",
    );
    chat_registry.stop("p-registration-race", "desktop");
}

#[test]
fn tailer_skips_when_provider_returns_no_jsonl_path() {
    // Register a noop provider that doesn't implement session_jsonl_path. The tailer thread should bail out cleanly without panicking.
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };

    struct NoopProv;
    impl AgentProvider for NoopProv {
        fn id(&self) -> &str {
            "noop"
        }
        fn display_name(&self) -> &str {
            "Noop"
        }
        fn binary_name(&self) -> &str {
            "noop"
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::from_millis(0),
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        // session_jsonl_path uses the default impl → returns None.
    }

    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(NoopProv));
    let reg = Arc::new(reg);

    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let handle = spawn_tailer(
        TailConfig {
            tab_id: "p-noop".into(),
            provider_id: "noop".into(),
            cwd: "/anything".into(),
            session_id: "x".into(),
            start_offset: 0,
        },
        reg,
        sink.clone() as Arc<dyn DeltaSink>,
    );

    // Tailer stops itself when `session_jsonl_path` is None.
    // If shutdown can't join within test timing, it indicates a stuck thread.
    std::thread::sleep(Duration::from_millis(500));
    handle.shutdown();

    // No messages should have been emitted.
    assert_eq!(sink.msgs.lock().len(), 0);
}

// Dropping TailerHandle must stop polling. Without this, orphan tailers keep emitting during remount races.
#[test]
fn dropping_handle_stops_the_raw_tailer() {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counter {
        n: AtomicUsize,
    }
    impl crate::service::tab_chat::RawTextSink for Counter {
        fn emit(&self, _tab_id: &str, _text: String) {
            self.n.fetch_add(1, Ordering::Relaxed);
        }
    }

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, b"line1\n").unwrap();

    let counter = Arc::new(Counter {
        n: AtomicUsize::new(0),
    });
    let sink: Arc<dyn crate::service::tab_chat::RawTextSink> = counter.clone();
    let handle = crate::service::tab_chat::spawn_raw_tailer(
        crate::service::tab_chat::RawTailConfig {
            tab_id: "p".into(),
            path: path.clone(),
            start_offset: 0,
            poll_interval: Duration::from_millis(40),
        },
        sink,
    );

    assert!(
        wait_until(2000, || counter.n.load(Ordering::Relaxed) >= 1),
        "tailer should emit the initial content"
    );
    let before = counter.n.load(Ordering::Relaxed);

    // Drop the handle — its thread must stop, NOT keep polling.
    drop(handle);
    std::thread::sleep(Duration::from_millis(150));

    // A live thread would pick this up; a stopped one won't.
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"line2\n").unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(
        counter.n.load(Ordering::Relaxed),
        before,
        "dropped tailer must stop emitting"
    );
}
