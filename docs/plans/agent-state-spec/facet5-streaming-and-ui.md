# Facet 5: Real-Time Streaming + UI Unlocks

## Thesis

PTY agents can feel like native chat agents when provider plugins classify both:

- parked lifecycle state: `working`, `clarifying_question`, `rate_limited`, ...
- live stream regions: "assistant prose is currently visible", "tool call chrome is visible", "turn is done"

Core must not parse provider TUI copy. Core maintains byte offsets, VT screen snapshots, stable message ids, and UI events. Provider plugins own interpretation.

## Existing Seams

- Raw bytes: `OutputBuffer.append/read_since`, absolute `offset`, capped full history.
- Rendered truth: `ScreenState.process/tail_text/get_content_hash`, VT100-aware, handles redraws better than raw text.
- Current lifecycle verdict: `classifyPaneState(snapshot) -> PaneStateClassification`.
- Chat stream event: `pane-chat-msg-{paneId}` carries normalized `ChatMsg[]`; frontend merges by `id`.
- ChatBackend precedent: assistant messages already grow in place by stable id.

## New Capability

Add an optional provider capability adjacent to `classifyPaneState`:

```ts
classifyPaneStream(snapshot: PaneStreamSnapshot): PaneStreamClassification | null
```

```ts
interface PaneStreamSnapshot {
  paneId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  turnId: string;
  rawOffset: number;
  rawChunkText: string;
  screenText: string;
  screenHash: string;
  previous?: PaneStreamMemory;
}

interface PaneStreamMemory {
  phase: "idle" | "assistant_text" | "tool_call" | "tool_result" | "thinking" | "prompt" | "unknown";
  visibleText?: string;
  blockId?: string;
  providerCursor?: unknown;
}

interface PaneStreamClassification {
  phase: PaneStreamMemory["phase"];
  confidence: number;
  messages?: LiveChatPatch[];
  memory?: PaneStreamMemory;
  turnDone?: boolean;
}

interface LiveChatPatch {
  id: string;
  type: "assistant" | "thinking" | "tool_call" | "tool_result" | "system";
  content: string;
  timestamp: string;
  provider: string;
  partial: boolean;
  blockId?: string;
  toolName?: string;
  toolArgs?: string;
  toolInput?: unknown;
  toolId?: string;
  collapsed?: boolean;
}
```

Rules:

- `id` is stable while a visible block grows: `{paneId}:{sessionId}:{turnId}:{blockId}`.
- A newer patch with the same `id` replaces prior content in the UI merge path.
- `partial=false` seals a block; sealed blocks must not be mutated except by a durable transcript reconciliation.
- `turnDone=true` closes the live turn and allows the next assistant text to mint a new `turnId`.
- Confidence below the global threshold emits no patch unless the provider can prove a harmless "typing" phase.

## Core Boundary

Core owns:

- subscribing to PTY chunks after `OutputBuffer.append` and `ScreenState.process`;
- constructing `PaneStreamSnapshot`;
- keeping per-pane `PaneStreamMemory`;
- debouncing/chunk coalescing, e.g. emit at most every 50-100 ms;
- emitting `pane-chat-msg-{paneId}` with `LiveChatPatch` mapped to `ChatMsg`;
- reconciling live partials with durable JSONL/sqlite transcript messages;
- clearing partials on pane close, session change, reset, or provider `turnDone`.

Provider owns:

- detecting which screen rows are assistant prose vs TUI chrome;
- detecting tool-call/tool-result regions and extracting tool labels;
- detecting backtracking/redraw semantics from the provider's TUI layout;
- declaring turn boundaries;
- refusing to emit when the stream is ambiguous.

Core must never match provider-specific strings such as `Update available`, `Esc to interrupt`, `thinking`, or menu text outside provider capability calls.

## Redraw Handling

Use rendered screen snapshots as canonical, raw chunks only as a trigger.

Algorithm:

1. PTY bytes arrive.
2. Core appends bytes and updates `ScreenState`.
3. Core builds `screenText`, `screenHash`, `rawOffset`, `rawChunkText`.
4. Provider returns the current visible semantic block, not an append delta.
5. Core emits a replacement patch for the same message id.

This avoids duplicate text when a TUI redraws the same paragraph, wraps lines, erases a spinner, or moves the cursor backward.

Provider extractors should normalize:

- CRLF to LF;
- soft wraps into spaces only inside provider-known prose regions;
- spinner/status rows out of assistant text;
- ANSI redraw artifacts by reading `screenText`, not raw escape bytes.

## Example: Assistant Prose

Visible screen:

```text
I found the restore bug. The pane session id is updated after the model swap,
but the chat tailer still points at the old session file.
```

Provider:

```json
{
  "phase": "assistant_text",
  "confidence": 0.93,
  "messages": [{
    "id": "pane1:sid1:t42:text0",
    "type": "assistant",
    "content": "I found the restore bug. The pane session id is updated after the model swap,\nbut the chat tailer still points at the old session file.",
    "timestamp": "1781900000000",
    "provider": "claude",
    "partial": true,
    "blockId": "text0"
  }],
  "memory": { "phase": "assistant_text", "blockId": "text0", "visibleText": "I found..." }
}
```

Next redraw reflows the same text. Provider returns the whole canonical visible block with the same id. UI replaces in place.

## Example: Tool Call

Visible screen:

```text
⏺ Bash(git status --short)
```

Provider:

```json
{
  "phase": "tool_call",
  "confidence": 0.91,
  "messages": [{
    "id": "pane1:sid1:t42:tool0",
    "type": "tool_call",
    "content": "Bash",
    "toolName": "Bash",
    "toolArgs": "git status --short",
    "toolInput": { "command": "git status --short" },
    "timestamp": "1781900000100",
    "provider": "claude",
    "partial": false,
    "collapsed": true,
    "blockId": "tool0"
  }],
  "memory": { "phase": "tool_call", "blockId": "tool0" }
}
```

Tool calls should seal early (`partial=false`) once the provider sees the command line. Tool result rows are separate blocks.

## Done Detection

`classifyPaneState` remains the parked-state authority. `classifyPaneStream` may set `turnDone=true` only when it sees a provider-owned done marker with high confidence.

Core closes a live turn when either:

- `classifyPaneStream.turnDone=true`, or
- `classifyPaneState` on the parked edge returns `done_idle`, `clarifying_question`, `rate_limited`, `waiting_on_auth`, `update_prompt`, or `idle_stuck` once the enum grows to include `done_idle`.

Until `done_idle` exists, use `PaneStatusTracker` transition `Working -> Idle/Waiting` plus provider stream memory to seal outstanding partials as "provisional done".

## Durable Transcript Reconciliation

Live PTY patches are optimistic projections. Durable provider session stores still win.

When `parse_session_delta` later emits the real message:

- if the real message matches a live partial by provider ids, content prefix, timestamp window, or tool id, replace the live patch id with the durable id;
- if it conflicts, mark the live patch superseded and hide it from the visible timeline;
- never render both the live projection and durable row as separate assistant bubbles.

Required `ChatMsg` additions:

```ts
partial?: boolean;
source?: "pty_live" | "session_store" | "chat_backend";
supersedes?: string[];
```

## UI Contract

`PaneSessionChat` consumes live PTY patches through the same `pane-chat-msg-{paneId}` event path as chatBackend streams.

Rendering rules:

- `partial=true`: show the assistant bubble growing in place; subtle typing affordance inside the bubble.
- `phase=thinking`: render collapsed by default.
- `phase=tool_call` / `tool_result`: render as tool blocks, not assistant prose.
- parked states from `pane-state-surface`: render interactive cards above the composer.
- input composer disabled only for auth/update/rate-limit states that are not answerable by normal text.

## State Model Unlock

Expose one frontend-facing status object:

```ts
interface PaneAttentionState {
  paneId: string;
  lifecycle:
    | "working"
    | "streaming_assistant"
    | "using_tool"
    | "done_idle"
    | "clarifying_question"
    | "options"
    | "waiting_on_auth"
    | "rate_limited"
    | "update_prompt"
    | "idle_stuck"
    | "unknown";
  confidence: number;
  since: string;
  resetAt?: string;
  questionCount?: number;
  optionCount?: number;
  surface?: { kind: string; payload: unknown };
}
```

This object is emitted on lifecycle changes and stored in mux pane state for sidebar/tooltips/restored windows.

## Ranked UI + State-Machine Features

1. Live assistant streaming in Chat UI
   - Highest value. Users see prose as the agent writes it instead of waiting for session JSONL flush or turn completion.

2. Accurate status pill per pane
   - `Working`, `Typing`, `Using tool`, `Needs answer`, `Rate limited until 16:20`, `Auth needed`, `Done`.
   - Drives sidebar dot, tab tooltip, chat header, and notification severity.

3. Interactive question/options cards
   - `clarifying_question` and `options` surfaces become answer cards with buttons/selectors.
   - Answers route through the existing `write_pty_string(raw=false)` path and create optimistic user bubbles.

4. Auto-advance / auto-resume
   - Skip update prompts, confirm safe wait options, schedule rate-limit resume, and resume idle-stuck panes.
   - All actions remain provider-scored and auth-guarded.

5. Done / await affordance
   - When `done_idle`, composer placeholder becomes "Ask follow-up".
   - When `idle_stuck`, show "Resume" / "Nudge" action.

6. Typing indicator without fake bubbles
   - Before enough text is visible, show provider avatar + "typing" based on harmless stream phase.
   - Replace it with the first partial assistant message once confidence is high.

7. Notification routing
   - Notify only for attention states: auth, question, rate-limit, idle-stuck, error.
   - Suppress notifications for normal streaming/tool phases.

8. Orchestrator state machine
   - Workers can be monitored by lifecycle instead of raw silence.
   - Parent can distinguish "still typing", "needs user", "rate limited", and "done".

## Fast Tests

Provider fixture tests:

- input: raw chunk series + expected rendered screen snapshots;
- expected: phase, confidence, stable message id, canonical content, `partial`, `turnDone`;
- negatives: diffs, shell prompts, menus, status bars, prose containing words like "wait" or "tool" that are not state markers.

Core unit tests:

- same id replaces message in place;
- redraw/reflow does not duplicate text;
- `turnDone` seals partials;
- session-store reconciliation hides live duplicate;
- low-confidence stream verdict emits nothing.

Frontend tests:

- `pane-chat-msg-{paneId}` with same id updates one bubble;
- `partial=true` renders as streaming state;
- tool call patch renders as tool block;
- `pane-state-surface` renders a card and answer routes through `write_pty_string`.

Performance test:

- replay a 2 MB PTY fixture at chunk boundaries;
- assert classifier calls are bounded, no async-runtime blocking, and UI event rate is coalesced.

## Risks

- Provider TUI layout changes: fixtures must be cheap to add, and weak matches must emit nothing.
- Live projection conflicts with durable JSONL: session store wins; live ids are superseded.
- Redraw flicker: provider returns current canonical visible block; core replaces by id.
- Hidden terminal panes: core can keep stream classification active only while Chat UI is mounted, or downsample to status-only when hidden.
- Privacy/noise: do not persist live PTY projections as durable history unless the provider session store later confirms them.

## Implementation Order

1. Add `partial/source/supersedes` to normalized chat message shape and merge logic.
2. Add core live-stream projector with no provider implementation; test id replacement and reconciliation.
3. Implement Claude prose/tool fixtures first; then Codex; then Opencode/Cursor.
4. Wire `PaneAttentionState` to sidebar and Chat UI header.
5. Replace terminal-text polling in `PaneSessionChat` with event-driven attention/stream events.

<!-- revision: 1 -->
