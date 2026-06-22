# chat-roundtrip — Layer B agent-driven chat round-trip harness

The **oracle** for "did my chat message actually land". Spawns a REAL provider
agent (claude / codex) on the **dev** daemon, sends a message through the same
seam the chat composer uses, and polls the **parsed transcript** until the user
echo AND a following assistant reply appear — then asserts both. This is the
backend-delivery proof that was missing when the OSC4 merge shipped unit-green
but chat-broken (facet8 §3, Layer B).

It is also the capture rig for the optimistic-bubble ("stuck on sending…")
diagnosis: it records, per provider, the exact `(client_send_ms, transcript
user-echo timestamp, content)` tuple, which `analyze-optimistic.mjs` then feeds
through the real `matchedOptimisticEchoes` matcher.

## Layers (facet8 §3)

- **Layer A — Playwright over the live WebView2 UI (ROG/Windows).** The pixel
  oracle: type into the composer, assert the user bubble appears, the optimistic
  "sending…" CLEARS, the assistant reply renders, the pill transitions. **Out of
  scope here** — runs on ROG.
- **Layer B — this harness (macOS/CI).** Same round-trip invariants minus
  pixels, via the REST API. No UI. Fast, runs anywhere dev runs.

## Run

```bash
cd evals/chat-roundtrip
./run.sh                        # claude + codex, elementary + rapid-send cases
CR_PROVIDERS="claude" ./run.sh  # one provider
CR_TIMEOUT=120 ./run.sh         # per-turn poll budget (seconds), default 90
CR_KEEP=1 ./run.sh              # leave panes/workspace for inspection
```

Dev-locked: `run.sh` sources `evals/_lib/lib.sh`, so every CLI/HTTP call is
pinned to dev (port 7686). It NEVER touches prod (7685). Requires a running dev
daemon (`./build.sh dev`).

## What it asserts (case ladder)

1. **session-binding (CE3, the gate)** — a FRESH pane must bind its session and
   deliver the transcript through the UI seam (`POST /chat-raw/start`, the
   `chat_start_core` path the web chat view uses). Send a nonce; poll
   `chat-raw/start` until `pending:false` AND the user echo is present. A perpetual
   `pending:true` is the exact bug Andrey saw — "Polo session: -", empty chat while
   the TUI works — so this case FAILS loudly on it. Per provider.
2. **elementary** — send one nonce prompt; assert (a) the user message is in the
   parsed transcript, (b) an assistant reply followed. Beats the "sending…"
   bug's backend half: proves the message reached the agent and drew a reply.
3. **rapid multi-send** — three quick distinct messages; assert all three land
   in the transcript (each as its own turn, or covered within a combined turn).

The send path is `POST /api/panes/{id}/send {text, enter:true}` →
`send_text_when_ready`, the identical seam the Tauri `chat_send_text` command
uses. The read path is `GET /api/panes/{id}/chat?since=0` — the same normalized
`ChatMsg` shape the chat UI renders.

> Note on PTY agents: the desktop composer types claude/codex sends straight
> into the PTY (`write_pty_string`), while `/send` routes through
> `send_text_when_ready`. Both deliver to the same agent and land the same
> transcript turn; Layer B uses `/send` because it is the deterministic,
> headless-drivable seam. The keystroke-vs-paste nuance is a Layer A concern.

## Files

- `run.sh` — the harness (spawn → send → poll → assert).
- `rt_lib.py` — transcript assertions (kept out of bash: macOS bash 3.2 cannot
  parse here-docs nested in command substitution).
- `analyze-optimistic.mjs` — feeds the captures through `matchedOptimisticEchoes`
  and prints, per provider, whether the real echo clears its bubble + the
  send-to-echo skew. The Part B diagnosis tool.

## Output / captures

`target-check/evals/chat-roundtrip/roundtrip-<provider>.json`:

```json
{ "user_echo_timestamp": "...", "user_echo_content": "...",
  "client_send_ms": 1781959405076, "assistant_replied": true }
```

## Part B finding (recorded here for provenance)

Real captures (this harness, macOS):

| provider | transcript user-echo timestamp | send→echo skew | matcher clears bubble |
|----------|--------------------------------|----------------|-----------------------|
| claude   | ISO-8601 UTC (`…Z`)            | +1503 ms       | YES                   |
| codex    | ISO-8601 UTC (`…Z`) — CE3 fix  | +1515 ms       | YES (by time now)     |

> CE3 update: codex previously emitted a **null** transcript timestamp (the plugin
> set only `ts` seconds, never the `timestamp` ISO string that flows to
> `ChatMsg.timestamp`), forcing content-only echo matching. The codex plugin now
> forwards the raw ISO `timestamp` exactly like claude (parity-gated in
> `parity_test/codex.rs`), so codex echoes match by time, not content alone.

So the `ECHO_SKEW_MS` time-guard does **not** reject fresh echoes for today's
providers — the matcher is sound for current data. But the seam is one
forwarding-line from breaking: `RichMessage.ts` is documented in Unix **seconds**
and the codex plugin computes `Math.floor(date/1000)`. If a seconds value ever
reaches the matcher, the pre-fix `timestampMs` read it as epoch-ms (1970) — ~56
years before the bubble's ms send-time — so the guard rejected every real echo
and the bubble stuck on "sending…" forever. The fix normalizes seconds→ms by
magnitude (`packages/chat-core/src/time.ts`), covered by `dedupe.test.ts` 18–21.

The full UI-bubble-clears proof is **Layer A (ROG Playwright)** — out of scope
here. Layer B proves the backend round-trip; the dedupe unit tests prove the
matcher logic.
