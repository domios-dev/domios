# Empty-Pane Chat UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chat UI opens on fresh agent panes (no JSONL session yet) and binds the session when it appears; "no session" becomes a typed `pending` state instead of an error.

**Architecture:** `chat_start_core` returns `Ok(ChatStartResult { pending: true, .. })` for a sessionless agent pane instead of `Err`. `PaneSessionChat` renders the normal empty chat on `pending` and re-runs `chat_start` when the mux snapshot delivers `agent_session_id`. `TerminalPane.onError` stops writing the client-mode store (auto-fallback ≠ user choice).

**Tech Stack:** Rust (axum/tauri shared core), React/TypeScript.

Spec: `docs/superpowers/specs/2026-06-07-empty-pane-chat-ui-design.md`

---

### Task 1: Rust — `pending` result for sessionless panes

**Files:**
- Modify: `src-tauri/src/commands/pane.rs:229-236` (struct), `:249-262` (gate)

- [ ] **Step 1: Add `pending` to `ChatStartResult`**

In `src-tauri/src/commands/pane.rs`, change the struct:

```rust
pub struct ChatStartResult {
    /// Raw JSONL text from the recent window — frontend parses via the
    /// claude TS plugin's `parseJsonlSession`. The window is anchored to
    /// a `\n` boundary so the parser never sees a partial first line.
    pub text: String,
    pub start_offset: u64,
    pub offset: u64,
    /// Agent pane exists but its session hasn't been created yet — render an empty chat and retry once the mux reports a session id.
    #[serde(default)]
    pub pending: bool,
}
```

- [ ] **Step 2: Return `pending` instead of `Err` in `chat_start_core`**

Replace the session extraction inside the first lock block:

```rust
    let (provider_id, cwd, mut session_id, mux_ms) = {
        let t = std::time::Instant::now();
        let m = svc.mux.lock();
        let pane = m.get_pane(&pane_id).ok_or("pane not found")?;
        let provider = pane.agent_type.clone().ok_or("pane has no agent_type")?;
        let cwd = pane.cwd.clone().unwrap_or_default();
        let Some(sid) = pane.agent_session_id.clone() else {
            // Fresh agent pane: the session file appears with the first agent reply.
            return Ok(ChatStartResult {
                text: String::new(),
                start_offset: 0,
                offset: 0,
                pending: true,
            });
        };
        (provider, cwd, sid, t.elapsed().as_millis())
    };
```

- [ ] **Step 3: Set `pending: false` at every other `ChatStartResult` construction site**

`grep -n "ChatStartResult {" src-tauri/src/commands/pane.rs` and add `pending: false,` to each remaining constructor (the success path at the end of `chat_start_core`, and any backfill/restart helpers the grep reveals).

- [ ] **Step 4: Compile**

Run: `./build.sh check`
Expected: no errors (warnings about unrelated code are fine).

- [ ] **Step 5: Integration check against dev**

```bash
./build.sh kill; ./build.sh dev --wait
TOKEN=$(grep -m1 '^token' ~/.codeterm-dev/config.toml | sed 's/.*= *"\(.*\)"/\1/')
AUTH="Authorization: Bearer $TOKEN"; BASE=http://127.0.0.1:7686
WS=$(curl -s -X POST -H "$AUTH" -H "Content-Type: application/json" -d '{"name":"chat-pending-test"}' $BASE/api/workspaces | jq -r .id)
PANE=$(curl -s -X POST -H "$AUTH" -H "Content-Type: application/json" -d '{"activate":true}' $BASE/api/workspaces/$WS/tabs | jq -r .pane_id)
curl -s -X POST -H "$AUTH" -H "Content-Type: application/json" -d '{"agent_type":"claude"}' $BASE/api/panes/$PANE/agent-type
curl -s -X POST -H "$AUTH" $BASE/api/panes/$PANE/chat-raw/start | jq
curl -s -X POST -H "$AUTH" $BASE/api/workspaces/$WS/close
```

Expected: `chat-raw/start` returns `{"text":"","start_offset":0,"offset":0,"pending":true}` (HTTP 200, not the previous 409 "pane has no session_id yet").

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/pane.rs
git commit -m "feat(chat): typed pending state for sessionless agent panes"
```

### Task 2: Frontend — render empty chat on `pending`, rebind on session arrival

**Files:**
- Modify: `src/components/Terminal/PaneSessionChat.tsx` (`ChatStartResult` interface ~line 68; `chat_start` effect ~line 403)

- [ ] **Step 1: Extend the local result interface**

```ts
interface ChatStartResult {
  text: string;
  offset: number;
  start_offset?: number;
  pending?: boolean;
}
```

- [ ] **Step 2: Rework the `chat_start` effect**

Replace the existing effect (currently keyed on `[paneId]`) with:

```tsx
  // A fresh agent pane reports pending=true (no JSONL session yet) — render
  // the normal empty chat (sends ride the PTY) and re-run this effect when
  // the mux snapshot delivers the session id.
  const agentSessionId = muxState.panes[paneId]?.agent_session_id ?? null;

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setError(null);
        setRawText(res.text || "");
        setEarliestOffset(res.start_offset ?? 0);
        setVisibleMessageCount(INITIAL_VISIBLE_MESSAGES);
        setLoading(false);
        if (res.pending) return;
        unlisten = await listen<RawDeltaEvent>(
          `pane-chat-raw-${paneId}`,
          (ev) => {
            if (cancelled) return;
            const newText = ev.payload.text || "";
            if (!newText) return;
            setRawText((prev) => prev + newText);
          },
        );
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      void invoke("chat_stop", { paneId }).catch(() => {});
    };
  }, [paneId, agentSessionId]);
```

Note: `muxState` is already in scope (used at ~line 313); place the
`agentSessionId` line directly above the effect.

- [ ] **Step 3: Type-check and build**

Run: `npx tsc --noEmit && pnpm build`
Expected: no TS errors, `✓ built`.

- [ ] **Step 4: Commit**

```bash
git add src/components/Terminal/PaneSessionChat.tsx
git commit -m "feat(chat): empty chat for pending sessions, rebind on session id"
```

### Task 3: Frontend — `onError` fallback must not write the client store

**Files:**
- Modify: `src/components/Terminal/TerminalPane.tsx:509-512`

- [ ] **Step 1: Drop the store write**

```tsx
            onError={() => {
              // Automatic fallback, NOT a user choice: never write the
              // client-mode store here or the pane gets pinned to terminal.
              setUiMode("terminal");
            }}
```

- [ ] **Step 2: Type-check and build**

Run: `npx tsc --noEmit && pnpm build`
Expected: clean. (`setClientPaneUiMode` stays imported — the explicit-toggle handler still uses it.)

- [ ] **Step 3: Commit**

```bash
git add src/components/Terminal/TerminalPane.tsx
git commit -m "fix(chat): error fallback no longer poisons client ui-mode store"
```

### Task 4: End-to-end verification

- [ ] **Step 1: Restart dev with fresh window**

Run: `./build.sh kill; ./build.sh dev --wait; pnpm build`

- [ ] **Step 2: Manual matrix (user)**

1. Desktop: new Claude pane → chat UI opens immediately, empty, composer focused.
2. Desktop: fresh pane, 3-dots → "Open Chat UI" → opens, no bounce to terminal.
3. Type a message pre-session → optimistic bubble; first agent reply back-fills history live.
4. Mobile web/PWA (relaunch app): new pane defaults to chat; same flow works.
5. Non-agent shell pane: no chat menu entry; nothing regressed.
