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
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tempfile::TempDir;

use crate::agent::chat_types::RichMessage;
use crate::agent::registry::AgentRegistry;
use crate::service::pane_chat::{spawn_tailer, DeltaSink, TailConfig};

struct Collector {
    msgs: Mutex<Vec<RichMessage>>,
}
impl DeltaSink for Collector {
    fn emit(&self, _pane_id: &str, messages: Vec<RichMessage>) {
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
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
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

#[test]
fn tailer_streams_new_messages_as_file_grows() {
    // Skip if running in an env without the plugin checkout.
    if !workspace_plugins_root().join("provider-claude").exists() {
        return;
    }

    // Lay out <tmp>/projects/<dash-encoded-cwd>/<sid>.jsonl so the
    // synthetic plugin (sessions.rootDir = tmp) resolves it correctly.
    let tmp = TempDir::new().unwrap();
    let cwd = "/proj/tail-demo";
    let sid = "sess-tail-001";
    // Dash-encoding: every non-alphanumeric becomes '-'. "/proj/tail-demo"
    // → "-proj-tail-demo".
    let proj_dir = tmp.path().join("-proj-tail-demo");
    std::fs::create_dir_all(&proj_dir).unwrap();
    let jsonl = proj_dir.join(format!("{sid}.jsonl"));

    // First write — must be picked up on the FIRST poll because the
    // tailer starts from offset 0.
    std::fs::write(
        &jsonl,
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    )
    .unwrap();

    // Build registry with sessions.rootDir pointing at our tempdir.
    let registry = registry_with_claude_at(tmp.path());
    let sink = Arc::new(Collector {
        msgs: Mutex::new(vec![]),
    });
    let handle = spawn_tailer(
        TailConfig {
            pane_id: "p1".into(),
            provider_id: "claude".into(),
            cwd: cwd.into(),
            session_id: sid.into(),
            start_offset: 0,
            poll_interval: Duration::from_millis(100),
        },
        registry,
        sink.clone() as Arc<dyn DeltaSink>,
    );

    assert!(
        wait_until(2000, || sink.msgs.lock().len() >= 1),
        "first message must arrive within 2s"
    );

    // Append a second line.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&jsonl)
        .unwrap();
    writeln!(
        f,
        "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"thanks\"}}}}"
    )
    .unwrap();
    drop(f);

    assert!(
        wait_until(2000, || sink.msgs.lock().len() >= 2),
        "second message must arrive within 2s"
    );

    {
        let msgs = sink.msgs.lock();
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    handle.shutdown();
}

#[test]
fn tailer_skips_when_provider_returns_no_jsonl_path() {
    // Register a noop provider that doesn't implement session_jsonl_path.
    // The tailer thread should bail out cleanly without panicking.
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
            pane_id: "p-noop".into(),
            provider_id: "noop".into(),
            cwd: "/anything".into(),
            session_id: "x".into(),
            start_offset: 0,
            poll_interval: Duration::from_millis(50),
        },
        reg,
        sink.clone() as Arc<dyn DeltaSink>,
    );

    // Tailer thread should exit on its own when session_jsonl_path is None.
    // We don't have a direct "did it exit" signal, but shutdown() joins
    // the thread — if it's stuck in a loop, the test hangs and that
    // surfaces the bug. Give it 500ms to fully spin up + return.
    std::thread::sleep(Duration::from_millis(500));
    handle.shutdown();

    // No messages should have been emitted.
    assert_eq!(sink.msgs.lock().len(), 0);
}

// Regression: dropping a TailerHandle must stop its polling thread. Without
// the `Drop` impl, ChatRegistry::insert's idempotent-skip (and remount races)
// leak orphan tailers that keep emitting — duplicating every chat message.
#[test]
fn dropping_handle_stops_the_raw_tailer() {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counter {
        n: AtomicUsize,
    }
    impl crate::service::pane_chat::RawTextSink for Counter {
        fn emit(&self, _pane_id: &str, _text: String) {
            self.n.fetch_add(1, Ordering::Relaxed);
        }
    }

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, b"line1\n").unwrap();

    let counter = Arc::new(Counter { n: AtomicUsize::new(0) });
    let sink: Arc<dyn crate::service::pane_chat::RawTextSink> = counter.clone();
    let handle = crate::service::pane_chat::spawn_raw_tailer(
        crate::service::pane_chat::RawTailConfig {
            pane_id: "p".into(),
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
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"line2\n").unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(
        counter.n.load(Ordering::Relaxed),
        before,
        "dropped tailer must stop emitting"
    );
}
