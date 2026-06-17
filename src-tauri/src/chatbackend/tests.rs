//! Completeness oracle for the pane↔PTY decoupling (plan Appendix B1.1#7).
//!
//! Two guards that "kill the fear" that a pane-keyed op was missed:
//!
//! 1. `chatbackend_registry_routes_every_op` — a fake [`ChatBackendProvider`]
//!    registered in the registry receives every transport op
//!    (open/send/poll/close/list) and no PTY is ever involved at this layer.
//!    The transport's chat impl (`ChatBackendTransport`) and the `ServiceContext`
//!    guards all funnel through exactly these registry calls.
//! 2. `no_new_resolve_pty_id_outside_allowlist` — the CI grep guard: a direct
//!    `resolve_pty_id(` call may only appear in the known PTY-path files. A new
//!    one elsewhere means a pane-keyed entry bypassed the `PaneTransport` seam.

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
    fn poll(&self, _session_id: &str) -> Result<Vec<ChatMsg>, ChatBackendError> {
        self.polled.fetch_add(1, Ordering::SeqCst);
        Ok(vec![ChatMsg {
            id: "1".into(),
            kind: "assistant".into(),
            content: "hi".into(),
            ..Default::default()
        }])
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
        }])
    }
}

#[test]
fn chatbackend_registry_routes_every_op() {
    let reg = ChatBackendRegistry::new();
    let fake = Arc::new(FakeBackend::default());
    reg.register("fake", fake.clone());

    let p = reg.get("fake").expect("registered backend resolvable");
    // Every pane-keyed op a chatBackend pane performs, end to end through the
    // capability layer — none of these touch a PTY.
    let sid = p.open_session(serde_json::json!({})).unwrap();
    assert_eq!(sid, "sess-1");
    p.send_message(&sid, "hello").unwrap();
    let msgs = p.poll(&sid).unwrap();
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

/// The CI grep guard (B1.1#7). A direct `resolve_pty_id(` call is allowed only
/// in these PTY-path files. Re-grep + update this list intentionally if a new
/// PTY-path file legitimately needs it — but a pane-keyed *entry* op must route
/// through `PaneTransport`, not add a fresh `resolve_pty_id`.
#[test]
fn no_new_resolve_pty_id_outside_allowlist() {
    const ALLOWED: &[&str] = &[
        // The transport seam itself — the canonical home for resolve_pty_id.
        "service/pane_transport.rs",
        // PTY-path internals + the PTY entry methods (guarded for chat).
        "service/pty_io.rs",
        "service/pane.rs",
        "service/spawn.rs",
        // PTY-only / None-safe REST + bridge sites (each guarded or PTY-only).
        "api/pane_routes.rs",
        "api/mem_routes/items.rs",
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
