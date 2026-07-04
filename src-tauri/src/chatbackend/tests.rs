//! Completeness oracle for the pane↔PTY decoupling. Two guards: a fake
//! [`ChatBackendProvider`] receives every capability op (no PTY at this layer),
//! and a CI grep guard asserting `resolve_pty_id(` appears only in PTY-path files
//! (a new one elsewhere means an entry op bypassed the `PaneTransport` seam).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::agent::provider::ModelInfo;

#[derive(Default)]
struct FakeBackend {
    opened: AtomicUsize,
    sent: AtomicUsize,
    polled: AtomicUsize,
    closed: AtomicUsize,
    listed: AtomicUsize,
}

impl ChatBackendProvider for FakeBackend {
    fn id(&self) -> &str {
        "fake"
    }
    fn open_session(&self, _config: serde_json::Value) -> Result<String, ChatBackendError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok("sess-1".into())
    }
    fn send_message(&self, _session_id: &str, _text: &str) -> Result<(), ChatBackendError> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn poll(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        self.polled.fetch_add(1, Ordering::SeqCst);
        Ok(crate::agent::chat_types::StructuredChat {
            messages: vec![ChatMsg {
                id: "1".into(),
                kind: "assistant".into(),
                content: "hi".into(),
                ..Default::default()
            }],
            cursor: None,
        })
    }
    fn close_session(&self, _session_id: &str) -> Result<(), ChatBackendError> {
        self.closed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        self.listed.fetch_add(1, Ordering::SeqCst);
        Ok(vec![ModelInfo {
            id: "m".into(),
            display_name: "M".into(),
            description: None,
            available: None,
        }])
    }
}

#[test]
fn chatbackend_registry_routes_every_op() {
    let reg = ChatBackendRegistry::new();
    let fake = Arc::new(FakeBackend::default());
    reg.register("fake", fake.clone());

    let p = reg.get("fake").expect("registered backend resolvable");
    // capability layer — none of these touch a PTY.
    let sid = p.open_session(serde_json::json!({})).unwrap();
    assert_eq!(sid, "sess-1");
    p.send_message(&sid, "hello").unwrap();
    let msgs = p.poll(&sid, None).unwrap().messages;
    assert_eq!(msgs.len(), 1);
    p.close_session(&sid).unwrap();
    assert_eq!(p.list_models().unwrap().len(), 1);

    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
    assert_eq!(fake.sent.load(Ordering::SeqCst), 1);
    assert_eq!(fake.polled.load(Ordering::SeqCst), 1);
    assert_eq!(fake.closed.load(Ordering::SeqCst), 1);
    assert_eq!(fake.listed.load(Ordering::SeqCst), 1);
}

#[test]
fn unregistered_backend_resolves_to_none() {
    let reg = ChatBackendRegistry::new();
    assert!(reg.get("absent").is_none());
}

#[test]
fn chatbackend_describe_does_not_call_live_model_getters() {
    let fake = FakeBackend::default();
    let desc = fake.describe();

    assert!(desc.available_models.is_empty());
    assert_eq!(fake.listed.load(Ordering::SeqCst), 0);
}

/// Pane teardown / `pty kill` reach the plugin via `chatbackend_close` →
/// `close_session(sid)`; assert that path releases the session (no model/context leak).
#[test]
fn teardown_closes_the_plugin_session() {
    let reg = ChatBackendRegistry::new();
    let fake = Arc::new(FakeBackend::default());
    reg.register("fake", fake.clone());

    let sid = reg
        .get("fake")
        .unwrap()
        .open_session(serde_json::json!({}))
        .unwrap();
    assert_eq!(fake.closed.load(Ordering::SeqCst), 0);

    // The exact call `chatbackend_close(plugin, sid)` makes on pane teardown.
    reg.get("fake").unwrap().close_session(&sid).unwrap();
    assert_eq!(fake.closed.load(Ordering::SeqCst), 1);
}

/// CI grep guard: `resolve_pty_id(` is allowed only in these PTY-path files. Update
/// the list intentionally for a new PTY-path file — entry ops must route through `PaneTransport`.
#[test]
fn no_new_resolve_pty_id_outside_allowlist() {
    const ALLOWED: &[&str] = &[
        // The transport seam itself — the canonical home for resolve_pty_id.
        "service/pane_transport.rs",
        // PTY-path internals + the PTY entry methods (guarded for chat).
        // `pty_io` split (send / lifecycle): resolve_pty_id is defined in lifecycle.rs; send.rs holds the keystroke-injection helper.
        "service/pty_io/send.rs",
        "service/pty_io/lifecycle.rs",
        "service/pane.rs",
        "service/spawn.rs",
        // PTY-only / None-safe REST + bridge sites (each guarded or PTY-only).
        // commands/pane.rs: shell_command_snapshot — None-safe (.ok()) pty-keyed read, command sibling of pane_routes.
        "commands/pane.rs",
        // dev-only (cfg(debug_assertions)) cast-replay injection into a pane's OutputBuffer;
        // best-effort (if let Ok) PTY resolution for test/verification infra, not a real spawn path.
        "api/dev_cast_routes.rs",
        "api/pane_routes.rs",
        "api/pane_routes/send.rs",
        "api/mem_routes/items/mod.rs",
        "bridge/manager/mod.rs",
    ];

    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<String> = Vec::new();
    let mut stack = vec![src_root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let rel = path
                .strip_prefix(&src_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // This very test names the symbol; skip the guard file.
            if rel == "chatbackend/tests.rs" {
                continue;
            }
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            if body.contains("resolve_pty_id(") && !ALLOWED.contains(&rel.as_str()) {
                offenders.push(rel);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "new resolve_pty_id() call outside the PaneTransport allowlist — route \
         the pane-keyed op through svc.pane_transport() instead: {offenders:?}"
    );
}
