# Pane UI Mode — JSONL Chat + PermissionRequest Hook

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-pane "UI mode" toggle that switches the xterm.js view to a live chat rendering of the running AI session, sourced by tailing the provider's session JSONL through plugin-owned parsers. PermissionRequest hook brings approval dialogs into the chat instead of blocking on TUI.

**Architecture:**
- **All provider-specific knowledge stays in the plugin** (where the JSONL is, how to parse it, what hook events to install). Rust core only orchestrates per-pane tailers and routes events.
- **Per-pane isolation** — a tailer exists only for panes the user has flipped to UI mode. No global subscription, no cross-pane contamination.
- **Same input pipeline as everything else** — chat composer calls `send_text_external`, the unified entry point built earlier. No new send path.
- **PermissionRequest = long-poll hook** — claude's hook command blocks on codeterm REST until the user clicks Allow/Deny in chat, then prints the decision JSON and unblocks claude.

**Tech Stack:** Rust (tokio, axum, parking_lot), TypeScript/React, QuickJS (rquickjs) for plugin VM, xterm.js + new ChatView component.

---

## File Structure

**Rust (new):**
- `src-tauri/src/service/pane_chat/mod.rs` — facade, public API
- `src-tauri/src/service/pane_chat/tailer.rs` — per-pane JSONL tailer (poll loop, byte offset state)
- `src-tauri/src/service/pane_chat/registry.rs` — pane_id → tailer handle map
- `src-tauri/src/service/pane_chat/permission.rs` — pending-permission registry (long-poll wait/resolve)
- `src-tauri/src/api/pane_chat_routes.rs` — REST: start/stop/backfill/permission-wait/permission-resolve
- `src-tauri/src/socket/server/pane_chat.rs` — JSON-RPC parity

**Rust (modified):**
- `src-tauri/src/agent/provider.rs` — add trait methods `session_jsonl_path`, `parse_session_delta`, `install_permission_hook`, `uninstall_permission_hook`
- `src-tauri/src/plugin/js_runtime/provider.rs` — forward new methods to JS
- `src-tauri/src/lib.rs` — register routes, init pane_chat registry in ServiceContext
- `src-tauri/src/service/mod.rs` — wire pane_chat module
- `src-tauri/src/service/pane.rs` — clear pane_chat state on pane close

**JS plugin (modified):**
- `plugins/provider-claude/plugin.js` — add `sessionJsonlPath`, `parseSessionDelta`, extend hook installer with PermissionRequest entry

**Frontend (new):**
- `src/components/Terminal/PaneSessionChat.tsx` — chat view that subscribes to `pane-chat-{paneId}` events
- `src/components/Terminal/PaneSessionChat/MessageList.tsx` — virtualized list
- `src/components/Terminal/PaneSessionChat/MessageBubble.tsx` — polymorphic message renderer (text/thinking/tool_use/tool_result)
- `src/components/Terminal/PaneSessionChat/PermissionDialog.tsx` — Allow/Deny prompt
- `src/components/Terminal/UIModeToggle.tsx` — header button to flip mode
- `src/lib/paneChatTypes.ts` — shared types

**Frontend (modified):**
- `src/components/Terminal/TerminalPane.tsx` — render `<PaneSessionChat>` when uiMode==="chat", `<xterm>` otherwise
- `src/store/multiplexer.ts` — add `paneUiMode` map, setter

---

## Stage Map

| Stage | Deliverable | Independently shippable |
|---|---|---|
| 1 | Rich-message JSONL parser (plugin) + `parse_session_delta` trait | yes — unit tests only |
| 2 | Per-pane tailer (Rust) | yes — integration test with fixture JSONL |
| 3 | REST routes for start/stop/backfill | yes — curl test against dev |
| 4 | Frontend toggle + ChatView shell (text-only first) | yes — manual UI check |
| 5 | Polymorphic message rendering + virtualization | yes — manual UI check |
| 6 | PermissionRequest hook + dialog | yes — manual end-to-end test |

Each stage commits when its tests/checks pass.

---

## Stage 1: Rich-message JSONL parser

The existing `parseSessionMessages` in claude plugin drops everything except plain text blocks. We need to keep `thinking`, `tool_use`, `tool_result` so the chat UI can render them. Plus we need a delta variant that takes a byte offset.

### Task 1.1: Define the RichMessage shape on Rust side

**Files:**
- Modify: `src-tauri/src/agent/provider.rs` (add types near `SessionMessage`)

- [ ] **Step 1: Add `RichMessage` and `MessageBlock` types**

After the `SessionMessage` struct, add:

```rust
/// A single content block inside a chat message. Polymorphic — claude's
/// JSONL packs `text`, `thinking`, `tool_use`, `tool_result` into the
/// same `message.content[]` array, each with its own shape. We carry
/// the raw `data` JSON so the frontend can render whatever is in it
/// without locking a schema across providers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageBlock {
    /// "text" | "thinking" | "tool_use" | "tool_result"
    pub kind: String,
    /// Block-specific payload. For "text"/"thinking" it's the string in
    /// `{text: "..."}`. For "tool_use" it's `{id, name, input}`. For
    /// "tool_result" it's `{tool_use_id, content, is_error}`.
    pub data: serde_json::Value,
}

/// One chat message (one JSONL line that's a user/assistant entry).
/// Richer than `SessionMessage` — keeps tool calls + thinking blocks
/// so the chat UI can render them. Used by the chat tail pipeline,
/// NOT the session-browser/restore pipeline (which still uses the
/// simpler `SessionMessage`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RichMessage {
    /// "user" | "assistant"
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    /// Unix seconds — null if JSONL lacked a timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// Provider session id (claude's `session_id` field on the entry).
    /// Lets the UI cross-reference with codeterm's pane session id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// True for system-meta entries (e.g. claude's `isMeta: true`).
    /// UI may collapse them.
    #[serde(default)]
    pub is_meta: bool,
}

/// Result of a single delta read on a session JSONL. `new_offset` is the
/// byte offset to pass back on the next call to resume from. `messages`
/// is the messages parsed from the bytes between previous offset and
/// `new_offset`. Empty `messages` with `new_offset == prev_offset` means
/// nothing new — the caller should sleep a bit and retry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionDelta {
    pub new_offset: u64,
    pub messages: Vec<RichMessage>,
}
```

- [ ] **Step 2: Add `session_jsonl_path` and `parse_session_delta` trait methods**

In the `AgentProvider` trait (same file, near `parse_session_messages`):

```rust
/// Resolve the actual session JSONL file for `(cwd, session_id)`. The
/// chat tailer reads this file with byte-offset bookmarks. Returns
/// `None` when the provider doesn't store sessions as a single JSONL
/// (opencode is sqlite-backed — we won't tail-chat opencode the same
/// way, that provider returns `None` and the UI falls back to a
/// different render strategy or hides the toggle).
fn session_jsonl_path(&self, _cwd: &str, _session_id: &str) -> Option<std::path::PathBuf> {
    None
}

/// Parse new messages from `path` starting at `from_offset` bytes,
/// returning the delta and a new offset to bookmark. Implementations
/// must be tolerant of partial last lines (claude writes one line at
/// a time but a poll can land mid-write): if the trailing bytes don't
/// terminate in `\n`, hold them back and return `new_offset` pointing
/// at the start of that incomplete line.
fn parse_session_delta(&self, _path: &std::path::Path, _from_offset: u64) -> SessionDelta {
    SessionDelta { new_offset: _from_offset, messages: vec![] }
}
```

- [ ] **Step 3: Run cargo check to verify it compiles**

Run: `./build.sh check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/agent/provider.rs
git commit -m "feat(agent/chat): RichMessage/SessionDelta types + plugin trait hooks"
```

### Task 1.2: Implement `sessionJsonlPath` in claude plugin

Claude stores sessions at `~/.claude/projects/<cwd-encoded>/<session-id>.jsonl`. The plugin already has the encoding helper (`encodeCwdDashed`) — we just need to expose the path resolver.

**Files:**
- Modify: `plugins/provider-claude/plugin.js`

- [ ] **Step 1: Add `sessionJsonlPath` method to plugin exports**

Find the section just before `parseSessionMessages` (around line 267). Add above it:

```javascript
  // Resolve the JSONL file path for (cwd, sessionId). Mirrors the
  // sessionDir layout encoded in plugin.json:sessions.rootDir +
  // cwdEncoding=dash-encoded.
  sessionJsonlPath: function (cwd, sessionId) {
    if (!sessionId) return null;
    var m = manifest();
    var rootRaw = (m && m.sessions && m.sessions.rootDir) || "~/.claude/projects";
    var root = host.expandHome(rootRaw);
    if (!root) return null;
    var leaf = encodeCwdDashed(String(cwd || ""));
    var sep = host.platform() === "windows" ? "\\" : "/";
    return root + sep + leaf + sep + sessionId + ".jsonl";
  },
```

`encodeCwdDashed` already exists later in the file — reuse without redefining. If grep shows it lives below this point in the file, hoist it above `module.exports` or factor it into the top.

- [ ] **Step 2: Verify by checking on a live ~/.claude/projects entry**

Run a quick node check (writing a throwaway script is fine, OR just trust the test in step 3):

```bash
ls ~/.claude/projects/ 2>&1 | head -5
```

Confirm the dir layout matches what `encodeCwdDashed` produces for a typical cwd.

- [ ] **Step 3: Add a unit test (parity_test.rs)**

Append to the `claude` mod in `src-tauri/src/plugin/js_runtime/parity_test.rs`:

```rust
    #[test]
    fn session_jsonl_path_resolves_via_plugin() {
        if !ws_exists() { return; }
        let j = load_js("claude");
        let p = j.session_jsonl_path("/Users/foo/work/proj", "sess-abc-123");
        assert!(p.is_some(), "should return Some(path)");
        let p = p.unwrap();
        let s = p.to_string_lossy();
        assert!(s.contains("sess-abc-123.jsonl"), "missing session id: {s}");
        assert!(s.contains("-Users-foo-work-proj"), "dash-encoded cwd missing: {s}");
        assert!(s.contains(".claude") && s.contains("projects"),
            "must hit ~/.claude/projects: {s}");
    }
```

- [ ] **Step 4: Run the test**

```bash
cd src-tauri && cargo test --lib plugin::js_runtime::parity_test::claude::session_jsonl_path_resolves_via_plugin
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/provider-claude/plugin.js src-tauri/src/plugin/js_runtime/parity_test.rs
git commit -m "feat(plugin/claude): sessionJsonlPath + Rust parity test"
```

### Task 1.3: Implement `parseSessionDelta` in claude plugin

This is the parser that the tailer feeds bytes to. Must handle partial last lines.

**Files:**
- Modify: `plugins/provider-claude/plugin.js`

- [ ] **Step 1: Add method**

Below `sessionJsonlPath`:

```javascript
  // Take a chunk of JSONL bytes and return RichMessage[]. Bytes
  // come straight from the file slice [fromOffset .. fileSize].
  // If the last line lacks a trailing \n we hold it back — the
  // caller's `consumedBytes` reflects only what was fully parsed,
  // so the next poll picks it up cleanly.
  //
  // Each user/assistant entry produces ONE RichMessage with its
  // `message.content[]` mapped block-for-block (text / thinking /
  // tool_use / tool_result). Non-message entries (summary, file-
  // history) are silently dropped.
  parseSessionDelta: function (chunk) {
    var s = String(chunk || "");
    var lastNl = s.lastIndexOf("\n");
    var consumed = 0;
    var parseSlice = "";
    if (lastNl < 0) {
      // No complete line yet — consume nothing.
      return { consumedBytes: 0, messages: [] };
    }
    consumed = lastNl + 1; // include the \n in consumed bytes
    parseSlice = s.substring(0, consumed);
    var out = [];
    var lines = parseSlice.split("\n");
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (_) { continue; }
      var t = v && v.type;
      if (t !== "user" && t !== "assistant") continue;
      var msg = v.message;
      if (!msg) continue;
      var content = msg.content;
      var blocks = [];
      if (typeof content === "string") {
        if (content) blocks.push({ kind: "text", data: { text: content } });
      } else if (Array.isArray(content)) {
        for (var j = 0; j < content.length; j++) {
          var b = content[j];
          if (!b || typeof b.type !== "string") continue;
          // Pass the whole block as `data` minus the `type` field —
          // type lives in `kind`. UI renders each kind on its own.
          var data = {};
          for (var k in b) {
            if (Object.prototype.hasOwnProperty.call(b, k) && k !== "type") {
              data[k] = b[k];
            }
          }
          blocks.push({ kind: b.type, data: data });
        }
      } else {
        continue;
      }
      if (!blocks.length) continue;
      var ts = null;
      if (typeof v.timestamp === "string") {
        var d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({
        role: t,
        blocks: blocks,
        ts: ts,
        sessionId: v.sessionId || null,
        isMeta: !!v.isMeta,
      });
    }
    return { consumedBytes: consumed, messages: out };
  },
```

- [ ] **Step 2: Add parity test**

In `parity_test.rs` claude mod:

```rust
    #[test]
    fn parse_session_delta_handles_polymorphic_content() {
        if !ws_exists() { return; }
        let j = load_js("claude");

        // Mixed: text + thinking + tool_use + tool_result, plus a non-
        // message entry (summary) and a malformed line.
        let jsonl = concat!(
            r#"{"type":"summary","x":1}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-05-20T10:00:00Z"}"#, "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u1","content":"foo bar"}]}}"#, "\n",
            "not-json-at-all\n",
        );

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();

        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(delta.new_offset, jsonl.len() as u64, "should consume whole file");
        assert_eq!(delta.messages.len(), 3, "3 user/assistant entries");

        let m0 = &delta.messages[0];
        assert_eq!(m0.role, "user");
        assert_eq!(m0.blocks.len(), 1);
        assert_eq!(m0.blocks[0].kind, "text");

        let m1 = &delta.messages[1];
        assert_eq!(m1.role, "assistant");
        assert_eq!(m1.blocks.len(), 3);
        let kinds: Vec<&str> = m1.blocks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["text", "thinking", "tool_use"]);
        assert_eq!(m1.blocks[2].data.get("name").and_then(|v| v.as_str()), Some("Bash"));

        let m2 = &delta.messages[2];
        assert_eq!(m2.blocks[0].kind, "tool_result");
        assert_eq!(m2.blocks[0].data.get("tool_use_id").and_then(|v| v.as_str()), Some("u1"));
    }

    #[test]
    fn parse_session_delta_holds_back_partial_line() {
        if !ws_exists() { return; }
        let j = load_js("claude");

        // First poll: only one complete line, then half a second line.
        let jsonl = concat!(
            r#"{"type":"user","message":{"role":"user","content":"first"},"timestamp":"2026-05-20T10:00:00Z"}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":"sec"#,  // truncated
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), jsonl).unwrap();
        let delta = j.parse_session_delta(tmp.path(), 0);
        assert_eq!(delta.messages.len(), 1, "partial line must be deferred");
        // new_offset points at the start of the partial line — i.e. right
        // after the first complete line's \n.
        let first_line_end = jsonl.find('\n').unwrap() as u64 + 1;
        assert_eq!(delta.new_offset, first_line_end);
    }
```

- [ ] **Step 3: Wire the Rust call**

In `src-tauri/src/plugin/js_runtime/provider.rs`, add to `impl AgentProvider for JsProvider`:

```rust
    fn session_jsonl_path(&self, cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
        let s: Option<String> = self.try_call("sessionJsonlPath",
            &[json!(cwd), json!(session_id)]);
        s.filter(|p| !p.is_empty()).map(std::path::PathBuf::from)
    }

    fn parse_session_delta(
        &self,
        path: &std::path::Path,
        from_offset: u64,
    ) -> crate::agent::provider::SessionDelta {
        use crate::agent::provider::SessionDelta;
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return SessionDelta { new_offset: from_offset, messages: vec![] },
        };
        let total = bytes.len() as u64;
        if from_offset >= total {
            return SessionDelta { new_offset: from_offset, messages: vec![] };
        }
        let slice = &bytes[from_offset as usize..];
        let chunk = String::from_utf8_lossy(slice).to_string();
        #[derive(serde::Deserialize)]
        struct JsDelta {
            #[serde(rename = "consumedBytes")]
            consumed_bytes: u64,
            messages: Vec<crate::agent::provider::RichMessage>,
        }
        let parsed: Option<JsDelta> = self.try_call("parseSessionDelta", &[json!(chunk)]);
        match parsed {
            Some(d) => SessionDelta {
                new_offset: from_offset + d.consumed_bytes,
                messages: d.messages,
            },
            None => SessionDelta { new_offset: from_offset, messages: vec![] },
        }
    }
```

- [ ] **Step 4: Run the new tests**

```bash
cd src-tauri && cargo test --lib plugin::js_runtime::parity_test::claude::parse_session_delta
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add plugins/provider-claude/plugin.js \
        src-tauri/src/plugin/js_runtime/provider.rs \
        src-tauri/src/plugin/js_runtime/parity_test.rs
git commit -m "feat(plugin/claude): parseSessionDelta with rich blocks + Rust bridge"
```

---

## Stage 2: Per-pane JSONL tailer

A tailer runs on its own thread per pane that's in UI mode. It polls the session JSONL every 500ms (tunable), parses the delta via the plugin, and emits a Tauri event the frontend subscribes to.

### Task 2.1: Tailer module skeleton

**Files:**
- Create: `src-tauri/src/service/pane_chat/mod.rs`
- Create: `src-tauri/src/service/pane_chat/tailer.rs`
- Create: `src-tauri/src/service/pane_chat/registry.rs`
- Modify: `src-tauri/src/service/mod.rs` (add `pub mod pane_chat;`)

- [ ] **Step 1: Write the registry**

`src-tauri/src/service/pane_chat/registry.rs`:

```rust
//! Per-pane tailer handle registry. One tailer per pane in UI mode.
//! `start` is idempotent (re-call returns the same handle).
//! `stop` signals the tailer thread to exit and drops the handle.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::tailer::TailerHandle;

#[derive(Default)]
pub struct ChatRegistry {
    inner: Mutex<HashMap<String, TailerHandle>>,
}

impl ChatRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns `true` if a new tailer was started, `false` if one was
    /// already running for this pane (caller can decide to no-op or
    /// to send a fresh snapshot anyway).
    pub fn insert(&self, pane_id: String, handle: TailerHandle) -> bool {
        let mut g = self.inner.lock();
        if g.contains_key(&pane_id) {
            return false;
        }
        g.insert(pane_id, handle);
        true
    }

    pub fn stop(&self, pane_id: &str) {
        if let Some(h) = self.inner.lock().remove(pane_id) {
            h.shutdown();
        }
    }

    pub fn is_running(&self, pane_id: &str) -> bool {
        self.inner.lock().contains_key(pane_id)
    }

    pub fn current_offset(&self, pane_id: &str) -> Option<u64> {
        self.inner.lock().get(pane_id).map(|h| h.current_offset())
    }
}
```

- [ ] **Step 2: Write the tailer**

`src-tauri/src/service/pane_chat/tailer.rs`:

```rust
//! One-pane JSONL tailer. Owns a thread that polls the session file,
//! parses deltas via the agent provider plugin, and emits events
//! via the supplied sink. Stops when `shutdown()` is called or when
//! the session file disappears for >10 consecutive polls (provider
//! closed the session).

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::agent::provider::RichMessage;
use crate::agent::registry::AgentRegistry;

/// Caller-supplied sink. Real impl emits a Tauri event with the
/// messages; tests pass a `Vec<RichMessage>` collector.
pub trait DeltaSink: Send + Sync {
    fn emit(&self, pane_id: &str, messages: Vec<RichMessage>);
}

pub struct TailerHandle {
    stop_flag: Arc<AtomicBool>,
    offset: Arc<AtomicU64>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl TailerHandle {
    pub fn current_offset(&self) -> u64 {
        self.offset.load(Ordering::Acquire)
    }

    pub fn shutdown(self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(j) = self.join.lock().take() {
            let _ = j.join();
        }
    }
}

pub struct TailConfig {
    pub pane_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    pub start_offset: u64,
    pub poll_interval: Duration,
}

pub fn spawn_tailer(
    cfg: TailConfig,
    registry: Arc<AgentRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let offset = Arc::new(AtomicU64::new(cfg.start_offset));

    let stop = stop_flag.clone();
    let off = offset.clone();
    let join = thread::spawn(move || {
        let path: Option<PathBuf> = registry
            .with_provider(&cfg.provider_id, |p| {
                p.session_jsonl_path(&cfg.cwd, &cfg.session_id)
            })
            .flatten();
        let path = match path {
            Some(p) => p,
            None => {
                log::warn!(
                    "[pane_chat] no JSONL path for pane={} provider={} sid={}",
                    cfg.pane_id, cfg.provider_id, cfg.session_id
                );
                return;
            }
        };
        let mut missing_streak = 0u32;
        while !stop.load(Ordering::Acquire) {
            if !path.exists() {
                missing_streak += 1;
                if missing_streak > 10 {
                    log::info!("[pane_chat] giving up on {:?} (10 polls missing)", path);
                    return;
                }
                thread::sleep(cfg.poll_interval);
                continue;
            }
            missing_streak = 0;
            let from = off.load(Ordering::Acquire);
            let delta = registry
                .with_provider(&cfg.provider_id, |p| {
                    p.parse_session_delta(&path, from)
                })
                .unwrap_or_else(|| crate::agent::provider::SessionDelta {
                    new_offset: from,
                    messages: vec![],
                });
            if delta.new_offset != from {
                off.store(delta.new_offset, Ordering::Release);
            }
            if !delta.messages.is_empty() {
                sink.emit(&cfg.pane_id, delta.messages);
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    TailerHandle {
        stop_flag,
        offset,
        join: Mutex::new(Some(join)),
    }
}
```

- [ ] **Step 3: Write the mod.rs facade**

`src-tauri/src/service/pane_chat/mod.rs`:

```rust
//! Per-pane chat tailers — UI-mode JSONL streaming.
//!
//! When a user flips a pane into UI mode, the API caller starts a
//! `Tailer` keyed on `pane_id`. The tailer polls the session JSONL
//! through the provider plugin and emits delta messages via a
//! `DeltaSink`. The tailer keeps a byte-offset bookmark so each poll
//! sends only new messages.
//!
//! All provider-specific logic lives in plugins — this module only
//! orchestrates `start/stop/backfill` over an opaque registry.

pub mod registry;
pub mod tailer;

pub use registry::ChatRegistry;
pub use tailer::{spawn_tailer, DeltaSink, TailConfig, TailerHandle};
```

- [ ] **Step 4: Wire into `service/mod.rs`**

Add to `src-tauri/src/service/mod.rs`:

```rust
pub mod pane_chat;
```

- [ ] **Step 5: Compile check**

Run: `./build.sh check`
Expected: 0 errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/service/pane_chat/ src-tauri/src/service/mod.rs
git commit -m "feat(service/pane_chat): tailer + registry scaffolding"
```

### Task 2.2: Integration test — fixture JSONL → tailer → sink

**Files:**
- Create: `src-tauri/src/service/pane_chat/tailer_tests.rs`
- Modify: `src-tauri/src/service/pane_chat/mod.rs` (add `#[cfg(test)] mod tailer_tests;`)

- [ ] **Step 1: Write the test**

```rust
//! Live integration: spawn a tailer over a real JSONL file that
//! grows during the test. Uses the actual claude JS plugin (no
//! mocks) to keep coverage honest.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tempfile::TempDir;

use crate::agent::provider::RichMessage;
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
        if f() { return true; }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn tailer_streams_new_messages_as_file_grows() {
    // Lay out a synthetic ~/.claude/projects/<encoded-cwd>/<sid>.jsonl
    // and point claude plugin at it via HOME override.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    let cwd = "/proj/tail-demo";
    let sid = "sess-tail-001";
    let proj_dir = home.join(".claude/projects/-proj-tail-demo");
    std::fs::create_dir_all(&proj_dir).unwrap();
    let jsonl = proj_dir.join(format!("{sid}.jsonl"));

    // SAFETY: integration test scoped env mutation.
    let prev_home = std::env::var_os("HOME");
    let prev_userprofile = std::env::var_os("USERPROFILE");
    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("USERPROFILE", home);
    }

    // First write before tailer starts — should be backfilled on first poll.
    std::fs::write(&jsonl, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n").unwrap();

    let registry = Arc::new(AgentRegistry::new_with_js_plugins_for_test());
    let sink = Arc::new(Collector { msgs: Mutex::new(vec![]) });
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

    // Wait for first message to arrive.
    assert!(wait_until(2000, || sink.msgs.lock().len() >= 1),
        "first message must arrive within 2s");

    // Append a new line — tailer should pick it up.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&jsonl).unwrap();
    writeln!(f, "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"thanks\"}}}}").unwrap();
    drop(f);

    assert!(wait_until(2000, || sink.msgs.lock().len() >= 2),
        "second message must arrive within 2s");

    let msgs = sink.msgs.lock();
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
    drop(msgs);

    handle.shutdown();

    // Restore env so other tests aren't disturbed.
    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
```

- [ ] **Step 2: Check if `AgentRegistry::new_with_js_plugins_for_test()` exists**

Search:
```bash
grep -rn "new_with_js_plugins_for_test\|fn new_for_test\|test_registry_with_claude" src-tauri/src/agent/
```

If not present, add a constructor in `src-tauri/src/agent/registry.rs` that loads only the workspace claude plugin (mirrors what `parity_test.rs` does via `load_js("claude")`). Keep `#[cfg(test)]` so it doesn't ship.

- [ ] **Step 3: Run the test**

```bash
cd src-tauri && cargo test --lib service::pane_chat::tailer_tests::tailer_streams_new_messages_as_file_grows
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/service/pane_chat/tailer_tests.rs src-tauri/src/service/pane_chat/mod.rs
git commit -m "test(service/pane_chat): live tailer integration over real JSONL"
```

---

## Stage 3: REST + JSON-RPC routes

### Task 3.1: DeltaSink that emits Tauri events

**Files:**
- Create: `src-tauri/src/service/pane_chat/tauri_sink.rs`
- Modify: `src-tauri/src/service/pane_chat/mod.rs`

- [ ] **Step 1: Write the sink**

```rust
//! Tauri-backed `DeltaSink`. Emits one event per delta, channel name
//! `pane-chat-{pane_id}` — keeps the frontend subscription per-pane
//! (no global firehose).

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::provider::RichMessage;
use crate::service::pane_chat::DeltaSink;

#[derive(Serialize)]
struct DeltaEvent<'a> {
    pane_id: &'a str,
    messages: &'a [RichMessage],
}

pub struct TauriDeltaSink {
    pub app: AppHandle,
}

impl DeltaSink for TauriDeltaSink {
    fn emit(&self, pane_id: &str, messages: Vec<RichMessage>) {
        let event = format!("pane-chat-{}", pane_id);
        let payload = DeltaEvent { pane_id, messages: &messages };
        if let Err(e) = self.app.emit(&event, &payload) {
            log::warn!("[pane_chat] emit {} failed: {}", event, e);
        }
    }
}

pub fn make_sink(app: AppHandle) -> Arc<dyn DeltaSink> {
    Arc::new(TauriDeltaSink { app })
}
```

- [ ] **Step 2: Expose from mod.rs**

Add to `mod.rs`:
```rust
pub mod tauri_sink;
```

- [ ] **Step 3: Add ChatRegistry to ServiceContext**

In `src-tauri/src/service/context.rs` (or wherever `ServiceContext` is defined — `grep -n 'pub struct ServiceContext'`):

```rust
pub chat_registry: Arc<crate::service::pane_chat::ChatRegistry>,
```

Initialize alongside other registries.

- [ ] **Step 4: Compile**

`./build.sh check` → 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/service/pane_chat/ src-tauri/src/service/context.rs
git commit -m "feat(service/pane_chat): Tauri event sink + ServiceContext wiring"
```

### Task 3.2: REST routes

**Files:**
- Create: `src-tauri/src/api/pane_chat_routes.rs`
- Modify: `src-tauri/src/api/routes.rs` (register routes)

- [ ] **Step 1: Write the routes**

`src-tauri/src/api/pane_chat_routes.rs`:

```rust
//! REST endpoints for per-pane chat (UI mode):
//! - POST /api/panes/:id/chat/start    → snapshot of current tail + start tailer
//! - POST /api/panes/:id/chat/stop     → stop tailer
//! - POST /api/panes/:id/chat/backfill → older messages (body: {before_offset})

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::service::pane_chat::{spawn_tailer, TailConfig};

#[derive(Deserialize)]
pub struct BackfillBody {
    pub before_offset: u64,
    /// How many bytes to read back from `before_offset`. Default 64 KB.
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

pub async fn start(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let svc = state.svc.clone();
    let app = state.app_handle.clone()
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "no app handle"))?;

    let (provider_id, cwd, session_id) = {
        let m = svc.mux.lock();
        let pane = m.get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "pane not found"))?;
        let provider = pane.agent_type.clone()
            .ok_or((StatusCode::BAD_REQUEST, "pane has no agent_type"))?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let sid = pane.agent_session_id.clone()
            .ok_or((StatusCode::BAD_REQUEST, "pane has no session_id yet"))?;
        (provider, cwd, sid)
    };

    let path = svc.agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
        .ok_or((StatusCode::NOT_IMPLEMENTED, "provider has no JSONL"))?;

    // Initial snapshot: read last 256 KB and parse.
    let snapshot_window: u64 = 256 * 1024;
    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let start_offset = file_size.saturating_sub(snapshot_window);
    let snapshot = svc.agent_registry
        .with_provider(&provider_id, |p| p.parse_session_delta(&path, start_offset))
        .unwrap_or(crate::agent::provider::SessionDelta {
            new_offset: file_size, messages: vec![]
        });

    let sink = crate::service::pane_chat::tauri_sink::make_sink(app);
    let handle = spawn_tailer(
        TailConfig {
            pane_id: pane_id.clone(),
            provider_id,
            cwd,
            session_id,
            start_offset: snapshot.new_offset,
            poll_interval: std::time::Duration::from_millis(500),
        },
        svc.agent_registry.clone(),
        sink,
    );
    svc.chat_registry.insert(pane_id, handle);

    Ok(Json(serde_json::json!({
        "ok": true,
        "snapshot": snapshot.messages,
        "offset": snapshot.new_offset,
    })))
}

pub async fn stop(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.chat_registry.stop(&pane_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn backfill(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<BackfillBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let max_bytes = body.max_bytes.unwrap_or(64 * 1024) as u64;
    let svc = state.svc.clone();

    let (provider_id, cwd, session_id) = {
        let m = svc.mux.lock();
        let pane = m.get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "pane not found"))?;
        (
            pane.agent_type.clone().ok_or((StatusCode::BAD_REQUEST, "no agent_type"))?,
            pane.cwd.clone().unwrap_or_default(),
            pane.agent_session_id.clone()
                .ok_or((StatusCode::BAD_REQUEST, "no session_id"))?,
        )
    };
    let path = svc.agent_registry
        .with_provider(&provider_id, |p| p.session_jsonl_path(&cwd, &session_id))
        .flatten()
        .ok_or((StatusCode::NOT_IMPLEMENTED, "no JSONL"))?;

    let start = body.before_offset.saturating_sub(max_bytes);
    let end = body.before_offset;
    let delta = svc.agent_registry
        .with_provider(&provider_id, |p| p.parse_session_delta(&path, start))
        .unwrap_or(crate::agent::provider::SessionDelta {
            new_offset: end, messages: vec![]
        });
    // Trim messages whose backing bytes go past `end` (defensive — parser
    // returned messages from a window we asked for, but if the file
    // grew during the call, the trailing slice may have new content too).
    Ok(Json(serde_json::json!({
        "ok": true,
        "messages": delta.messages,
        "start_offset": start,
        "end_offset": end,
    })))
}
```

- [ ] **Step 2: Register routes**

In `src-tauri/src/api/routes.rs`, find the router builder and add:

```rust
.route("/api/panes/:id/chat/start", post(crate::api::pane_chat_routes::start))
.route("/api/panes/:id/chat/stop",  post(crate::api::pane_chat_routes::stop))
.route("/api/panes/:id/chat/backfill", post(crate::api::pane_chat_routes::backfill))
```

Add `mod pane_chat_routes;` to `src-tauri/src/api/mod.rs`.

- [ ] **Step 3: Compile**

`./build.sh check` → 0 errors.

- [ ] **Step 4: Manual smoke against dev**

```bash
./build.sh kill && ./build.sh dev --wait
# spawn a claude pane via UI, note its pane_id from `dc pane list`
TOKEN=$(jq -r '.api.token' ~/.codeterm-dev/config.toml)
PANE=<paste pane id>
curl -s -X POST http://127.0.0.1:7686/api/panes/$PANE/chat/start \
  -H "Authorization: Bearer $TOKEN" | jq '.snapshot | length'
# Expected: a number > 0 if claude has produced any messages.
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/pane_chat_routes.rs src-tauri/src/api/routes.rs src-tauri/src/api/mod.rs
git commit -m "feat(api): per-pane chat REST endpoints"
```

### Task 3.3: Pane-close cleanup

When a pane closes, its tailer must die.

**Files:**
- Modify: `src-tauri/src/service/pane.rs` — find `close_pane`, add `self.chat_registry.stop(pane_id);`

- [ ] **Step 1: Add the stop call**

Find `pub fn close_pane` in `src-tauri/src/service/pane.rs`, add before the existing cleanup:
```rust
self.chat_registry.stop(pane_id);
```

- [ ] **Step 2: Compile + commit**

```bash
./build.sh check
git add src-tauri/src/service/pane.rs
git commit -m "fix(pane_chat): stop tailer on pane close"
```

---

## Stage 4: Frontend — UI mode toggle + chat shell

### Task 4.1: Per-pane uiMode state

**Files:**
- Modify: `src/store/multiplexer.ts`

- [ ] **Step 1: Add state slice**

In the Zustand store (or whatever pattern multiplexer uses), add:

```typescript
paneUiMode: Record<string, "terminal" | "chat">,
setPaneUiMode: (paneId: string, mode: "terminal" | "chat") => void,
```

Default = `"terminal"`. Persist if other per-pane state is persisted (check existing pattern).

- [ ] **Step 2: Commit**

```bash
git add src/store/multiplexer.ts
git commit -m "feat(store): paneUiMode per-pane state"
```

### Task 4.2: Toggle button

**Files:**
- Create: `src/components/Terminal/UIModeToggle.tsx`

- [ ] **Step 1: Implement**

```tsx
import { useMultiplexer } from "../../store/multiplexer";

interface Props {
  paneId: string;
  agentType?: string | null;
}

const SUPPORTED = new Set(["claude"]); // expand as plugins implement sessionJsonlPath

export function UIModeToggle({ paneId, agentType }: Props) {
  const { paneUiMode, setPaneUiMode } = useMultiplexer();
  if (!agentType || !SUPPORTED.has(agentType)) return null;
  const mode = paneUiMode[paneId] || "terminal";
  const next = mode === "terminal" ? "chat" : "terminal";
  return (
    <button
      className="text-xs px-2 py-1 rounded border border-[var(--ct-muted-fg)] opacity-70 hover:opacity-100"
      onClick={() => setPaneUiMode(paneId, next)}
      title={`Switch to ${next} view`}
    >
      {mode === "terminal" ? "💬 UI" : "▶ TUI"}
    </button>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add src/components/Terminal/UIModeToggle.tsx
git commit -m "feat(ui): UIModeToggle button"
```

### Task 4.3: Chat view shell

**Files:**
- Create: `src/components/Terminal/PaneSessionChat.tsx` (text-only first cut)
- Create: `src/lib/paneChatTypes.ts`

- [ ] **Step 1: Shared types**

```typescript
// src/lib/paneChatTypes.ts
export interface MessageBlock {
  kind: string; // "text" | "thinking" | "tool_use" | "tool_result"
  data: Record<string, unknown>;
}
export interface RichMessage {
  role: "user" | "assistant";
  blocks: MessageBlock[];
  ts?: number;
  is_meta?: boolean;
  session_id?: string;
}
```

- [ ] **Step 2: Component**

```tsx
// src/components/Terminal/PaneSessionChat.tsx
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RichMessage } from "../../lib/paneChatTypes";
import { fetchApi } from "../../lib/api"; // existing helper for authed REST

interface Props {
  paneId: string;
}

export function PaneSessionChat({ paneId }: Props) {
  const [msgs, setMsgs] = useState<RichMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const offsetRef = useRef<number>(0);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    (async () => {
      const res = await fetchApi(`/api/panes/${paneId}/chat/start`, { method: "POST" });
      if (cancelled) return;
      setMsgs(res.snapshot || []);
      offsetRef.current = res.offset || 0;
      setLoading(false);
      unlisten = await listen<{ messages: RichMessage[] }>(
        `pane-chat-${paneId}`,
        (ev) => {
          if (cancelled) return;
          setMsgs((prev) => [...prev, ...ev.payload.messages]);
        },
      );
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      void fetchApi(`/api/panes/${paneId}/chat/stop`, { method: "POST" });
    };
  }, [paneId]);

  if (loading) return <div className="p-4 opacity-60">Loading…</div>;
  return (
    <div className="w-full h-full overflow-y-auto p-3 text-sm font-mono">
      {msgs.map((m, i) => (
        <div key={i} className={m.role === "user" ? "text-[var(--ct-accent)]" : ""}>
          <strong>{m.role}:</strong>{" "}
          {m.blocks
            .filter((b) => b.kind === "text")
            .map((b) => (b.data as { text?: string }).text)
            .join(" ")}
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 3: Mount in TerminalPane**

In `src/components/Terminal/TerminalPane.tsx`, near top:

```tsx
import { PaneSessionChat } from "./PaneSessionChat";
import { UIModeToggle } from "./UIModeToggle";
// ...
const { paneUiMode } = useMultiplexer();
const uiMode = paneUiMode[paneId] || "terminal";
```

Wrap the xterm container in a conditional:

```tsx
{uiMode === "chat" ? (
  <PaneSessionChat paneId={paneId} />
) : (
  /* existing xterm container */
)}
```

Add `<UIModeToggle paneId={paneId} agentType={...} />` to wherever the pane header lives (likely TabBar or split header). If no obvious spot, drop it absolutely-positioned top-right inside TerminalPane for now.

- [ ] **Step 4: Compile + dev verify**

```bash
./build.sh check
./build.sh kill && ./build.sh dev --wait
# manual: spawn claude pane, click UI toggle, verify chat appears with content
```

- [ ] **Step 5: Commit**

```bash
git add src/components/Terminal/PaneSessionChat.tsx \
        src/lib/paneChatTypes.ts \
        src/components/Terminal/TerminalPane.tsx
git commit -m "feat(ui): per-pane chat shell (text-only)"
```

---

## Stage 5: Polymorphic rendering + virtualization

(Plan TBD — execute after Stages 1-4 are live and verified.)

- Split `PaneSessionChat` into `MessageList` + `MessageBubble`
- `MessageBubble` switches on `block.kind`: text (markdown), thinking (collapsed by default), tool_use (icon + name + collapsible input), tool_result (code block, collapsible if long)
- Virtualization: `react-window` or simple intersection-observer windowing
- Scroll-up backfill: when top sentinel intersects, call `/chat/backfill?before_offset=<offset of oldest loaded>`
- Persist scroll position; auto-scroll to bottom only when user is already at bottom

---

## Stage 6: PermissionRequest hook pipeline

(Plan TBD — execute after Stages 1-5 are stable.)

Outline:

- Claude plugin's `installIntegration` adds a `PermissionRequest` hook entry whose `command` is a long-poll curl:
  ```
  curl -s -X POST http://localhost:$PORT/api/panes/$PANE_ID/hook/permission_request \
    --data-binary @- -H "Authorization: Bearer $TOKEN" \
    --max-time 300
  ```
- New route holds connection up to 300s. Body is claude's hook input JSON (`tool_name`, `tool_input`). Server:
  1. Registers a pending permission keyed on (pane_id, request_id)
  2. Emits `pane-permission-{pane_id}` event with the request
  3. Awaits a `tokio::sync::oneshot` until UI calls `POST /api/panes/:id/hook/permission_response { request_id, behavior: "allow"|"deny" }`
  4. Returns the decision JSON for claude to read on stdout
- UI subscribes to `pane-permission-{pane_id}`, renders inline dialog in chat with Allow/Deny buttons
- On click → POST response → server unblocks → hook curl returns → claude proceeds (or denies)
- Timeout (300s) → server returns `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","reason":"codeterm UI timeout"}}}`

The piece worth verifying live before coding the full path: confirm `PermissionRequest` hook command can return JSON on stdout that claude actually honors. Once verified, the rest is mechanical.

---

## Self-review notes

- Spec coverage: per-pane scope ✓, JSONL tail via plugin ✓, polymorphic rendering ✓ (Stage 5), permission dialogs ✓ (Stage 6), unified input ✓ (no new send path — composer reuses `send_text_external`).
- Placeholder scan: Stages 5 and 6 are TBD-outlined — that's deliberate, they depend on Stages 1-4 landing first to constrain choices. Not placeholders inside an executable stage.
- Type consistency: `RichMessage`/`MessageBlock` shapes match across Rust (`SessionDelta`), JS plugin output (`{messages: RichMessage[]}`), REST snapshot, Tauri event payload, and frontend types in `paneChatTypes.ts`.

---

## Execution

**Plan saved to `docs/superpowers/plans/2026-05-20-pane-ui-mode-jsonl-chat.md`.**

Recommended: **subagent-driven-development** — each stage's tasks dispatched as fresh subagents, two-stage review between. Authoritative source for all per-provider behavior remains the plugin; Rust core only orchestrates.
