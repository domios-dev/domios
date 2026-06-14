# Universal pane→session binding (provider-reported only)

<!-- Combined brainstorm: Lamarr (orchestrator) diagnosis + principle, Lamarr v2 (codex) provider mechanisms. -->

## Problem

A pane's chat-UI must always tail the EXACT agent session that pane is running — for claude, codex, and opencode — with no hardcode, no cwd/mtime guessing, no magic. Today this is inconsistent, and the inconsistency exactly predicts what works:

| Provider | How the session is identified today | Chat-UI |
|----------|-------------------------------------|---------|
| **claude** | the agent REPORTS its id (SessionStart hook → `POST /api/panes/{id}/hook/session_start` → `set_agent_session_id`) + preassigned `--session-id` | ✅ works |
| **opencode** | core reads opencode's sqlite (`session`/`message`/`part`) — `uses_structured_chat = true` | ✅ chat parser works, but binding still relies on `db_latest_session(cwd)` = a newest-cwd guess |
| **codex** | core SCRAPES a screen-visible uuid (`extract_visible_session_uuid`) AND has no `parse_session_delta` (trait default → 0 messages) | ❌ "pane has no session_id yet" + empty chat |

The detector family (`detect_session_id(cwd, mtime)`, `detect_started_session`) **cannot prove pane ownership when two panes share a cwd** — this is exactly the W3 failure mode. W3-B already made detection "refuse to guess" when ambiguous; the correct end-state is to stop using detection to ASSIGN a session id at all.

## Principle

**Provider-reported binding only.** Each provider reports its session id from INSIDE the running agent process, carrying the CodeTerm pane id (and a per-launch nonce) from the pane environment. Core persists only reported (or explicitly preselected) ids. Filesystem/sqlite stores are read **by exact id**, never to choose ownership.

This is the user's "get it from the agent itself," made uniform across providers.

## Universal seam

Add a binding capability; stop letting `detect_*` write `pane.agent_session_id`.

```rust
pub struct SessionBindingReport {
    pub provider_id: String,
    pub pane_id: String,
    pub session_id: String,
    pub source: String,                 // startup | resume | clear | compact | created | select
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    pub store_path: Option<String>,
    pub binding_nonce: Option<String>,
}

pub trait AgentProvider {
    fn session_binding_capability(&self) -> SessionBindingCapability { SessionBindingCapability::None }
    fn ensure_session_binding_integration(&self, api_base: &str) -> Result<(), String> { Ok(()) }
    fn build_session_binding_env(&self, pane_id: &str, nonce: &str) -> Vec<(String, String)> { vec![] }
}
```

- Core asks the provider to install an authoritative reporter — never "newest session for cwd".
- Every provider returns the same `SessionBindingReport` shape.
- `detect_session_id` stays for the session browser/search only; it must NOT assign `pane.agent_session_id`.

**Generic report route** — generalize the existing `POST /api/panes/:pane_id/hook/session_start` to accept all providers:

```json
{ "provider_id": "codex", "session_id": "019ec20c-...", "source": "startup",
  "cwd": "...", "transcript_path": "...", "binding_nonce": "..." }
```

Route behavior: resolve pane → validate provider exists → set `agent_type` from `provider_id` if unset, else require match → if a per-launch nonce is on the pane, require report nonce to match → reject a session id already claimed by another pane (keep the atomic duplicate guard) → save `agent_session_id` (+ optional source/store_path/reported_at) → stop/restart the chat tailer/poller on id change → emit state changed.

**Env injected into every spawned + manual pane shell** (so manual `codex`/`opencode` launches still report the right pane): `CODETERM_PANE_ID` (exists), `CODETERM_SESSION_BINDING_NONCE`, `CODETERM_AGENT_PROVIDER`, `CODETERM_API_URL`, `CODETERM_API_TOKEN` (when set).

## Per-provider mechanisms

**Claude** — keep the SessionStart hook (final authority for `/new`/`/clear`); normalize its body onto `SessionBindingReport`; keep preassigned `--session-id` for an immediate known id on fresh launches.

**Codex** (cli 0.139.0; NO `--session-id` flag on fresh launch, only `codex resume <uuid>`) — bind via codex's official `SessionStart` hook (matchers `startup|resume|clear|compact`; stdin JSON has `session_id`, `transcript_path`, `cwd`, `source`, `model` — https://developers.openai.com/codex/hooks). Install `~/.codex/hooks.json` pointing at a CodeTerm CLI helper (portable over raw curl):
`codeterm hook session-start --provider codex --pane "$CODETERM_PANE_ID"` — reads stdin, forwards id/transcript/cwd/source/model + `CODETERM_SESSION_BINDING_NONCE`, POSTs to core. Trust caveat: non-managed codex hooks must be reviewed/trusted; surface integration health, never silently fall back to recency.

**Codex chat parser** (still needed, #165):
- `session_jsonl_path(cwd, sid)` → exact rollout path (`~/.codex/sessions/YYYY/MM/DD/rollout-...-<uuid>.jsonl`); resolve via `state_5.sqlite.threads` first, recursive filename fallback second.
- `parse_session_delta(path, offset)` → parse complete JSONL lines, map: `response_item.message` (user/assistant/developer) → text blocks; `response_item.reasoning` → thinking; `response_item.function_call` → tool_use; `response_item.function_call_output` → tool_result; ignore `event_msg.token_count`/UI noise. (First line is `session_meta`, `payload.id` is authoritative.)

**OpenCode** (1.17.4) — bind via a CodeTerm OpenCode server plugin (`@opencode-ai/plugin` `event` hook), NOT sqlite recency. Plugin reports `session.created` (→ `event.properties.info.id`), `tui.session.select`, and `message.updated`/`message.part.updated` as activity fallback, each carrying `process.env.CODETERM_PANE_ID` + provider + cwd + nonce. Core then reads sqlite by exact id (`~/.local/share/opencode/opencode.db`). `db_latest_session(cwd)` demoted to legacy/diagnostic.

## Edge cases

- **Fresh pane before first turn**: claude/codex hooks bind at startup; opencode returns chat `pending: true` until the plugin reports `session.created`. (Surface "waiting for provider session report", never empty.)
- **Multiple panes same cwd**: reports carry pane id + nonce; no cwd comparison; duplicate-assignment rejected atomically.
- **Restart/resume**: set saved id optimistically (user asked to resume it), provider report confirms or overrides; stale saved id + fresh start → pane pending until a fresh report.
- **`/new` `/clear` compact**: claude hook + codex `clear`/`compact` sources + opencode `session.created`/select — on id change, invalidate + restart the tailer/poller for the new exact id.
- **Provider integration absent/untrusted**: do not guess — show pending + binding-integration status (installed / trusted / last report / last error) in provider settings.

## Security

Require provider-id match + nonce when available; include auth header when a token exists; keep the route local-only; an arbitrary report must NOT switch a pane to a different provider.

## Related (fix alongside, separate root cause)

**Claude chat-UI live-update lag** — messages render "queued" and need a chat-UI reload to refresh. This is a live-streaming/SSE-push issue (the `pane-chat-msg` stream or optimistic reconcile), NOT session-identity. Investigate the tailer→sink→SSE live path for the claude pane separately; it is not solved by the binding rework.

## Implementation sequence

1. Generalize `hook_session_start` into the generic `SessionBindingReport` route (keep claude back-compat).
2. Inject per-launch binding nonce/env into backend PTY + frontend-created PTY paths.
3. Move claude hook onto the generic report shape.
4. Codex `SessionStart` integration + `codeterm hook session-start` CLI helper.
5. Codex `session_jsonl_path` + `parse_session_delta` (rollout → RichMessage) + parity test.
6. OpenCode plugin integration (`session.created` + select/activity).
7. Remove pane-session writes from `kick_off_session_detection`, `refresh_session_model_once`, `ensure_agent_state`, chat-start preflights (read exact ids only; never assign guessed ids).
8. Update REST `pane_chat_routes::start` to support structured providers / route through `chat_start_core`.
9. Tests/evals: two same-cwd codex panes → two distinct hook reports; two same-cwd opencode panes → two `session.created` reports; stale-saved-id-then-fresh-report; `/clear` report restarts tailer; missing hook/plugin → stays `pending`, never binds a sibling.

## Non-goals

- No binding inference from title, cwd, mtime, newest sqlite row, screen text, or prompt content.
- No core branching on `"codex"`/`"opencode"` to decide binding — provider ids live inside provider integrations + reports.
- sqlite/log/session files used only AFTER a provider-owned report gives the exact id.

## Scope note

Not a 1.4.1 cut blocker: claude + opencode chat already work; this reworks binding to be uniform and unblocks codex chat-UI. Target a follow-up release; 1.4.1 can cut independently.

<!-- revision: 1 -->
