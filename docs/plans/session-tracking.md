# Session Tracking Plan

> **Status (2026-04-13):** ~80% done. Tasks A (validate-before-resume), B1 (proactive UUID injection), B2 (session hooks), C (cross-provider search) DONE. Task B3 (fingerprint fallback) intentionally deferred — only needed if non-Claude providers regress.

Fix agent-session lifecycle across all providers: stop retrying stale sessions, detect which session belongs to which pane (even with multiple sessions in one cwd), and let users search past sessions from inside CodeTerm.

## Status

- **Task A** — DONE (`40de383`). Validate session file before `--resume`.
- **Task B1** — DONE (`f8b4d9a`). Proactive `--session-id` injection for Claude. Stress-tested with 5 panes in one cwd: each got a unique UUID, all restored correctly after restart.
- **Task B2** — DONE (`d590e9d`). Claude `SessionStart` hook + auth exemption for loopback hook callbacks. Covers `/new` and `/clear` inside the TUI.
- **Task B3** — DEFERRED. Content-fingerprint fallback for providers without injection + hooks. Only needed once we add a non-Claude provider that has session files but no way to announce its id.
- **Task C** — DONE (`dabd5c3`). Cross-provider session search via CLI + REST. Claude parses JSONLs; Codex still on the default stub (returns empty until its on-disk schema is audited).

## Context

Current state after `993f8e7`:
- Pane stores `agent_session_id` (set at launch, persisted in session snapshot)
- `ClaudeProvider::build_resume_command(session_id)` emits `claude --resume <uuid>`
- `ClaudeProvider::detect_session_id(cwd, exclude, max_age)` scans `~/.claude/projects/-<encoded>/*.jsonl` for newest
- `ClaudeProvider::session_file_path(cwd, id)` returns expected JSONL path

### Observed bugs
1. **Resume loop spam** — when the JSONL referenced by `agent_session_id` is gone (cleared cache, different machine, synced config without sessions), Claude prints `No conversation found with session ID: <uuid>` and exits. Watchdog restarts → same stale id → infinite loop.
2. **/new inside TUI** — user types `/new`, Claude creates a fresh session with a new uuid. Pane still points at the old one; on next restart we resume the wrong thread.
3. **Multiple sessions per cwd** — two Claude panes in the same workspace cwd both write JSONLs side-by-side. "Newest file" detection cannot tell which one is which pane.

---

## Task breakdown

### Task A — Validate session id before resume (quick fix)
**Scope:** any pane, any provider. Pane-generic, not just General Agent.

Before emitting a `--resume <id>` command:
- Call `provider.session_file_path(cwd, id)` — if the returned path doesn't exist, treat the session as gone
- Clear `pane.agent_session_id` (emit state change)
- Launch the agent fresh (no resume flag) — keep whatever other launch flags apply

**Files touched:**
- `src-tauri/src/commands.rs:~162` (resume flow in start-up restore)
- `src-tauri/src/service/general_agent.rs` (launch_command builder)
- `src-tauri/src/agent/provider.rs` (trait — add `fn session_exists(&self, cwd, id) -> bool { default via session_file_path }`)

**Acceptance:** missing JSONL → single clean launch, no spam. Loop stops.

---

### Task B — Proper session identification per pane

The hard part. We need a reliable mapping `pane_id → session_id` even when:
- Multiple panes share a cwd
- User runs `/new` inside the TUI
- Provider is not Claude (Codex, future agents)

#### B1 — Proactive id injection (primary strategy)
Generate a UUID on the CodeTerm side, pass it to the provider's launch command.

- Claude supports `--session-id <uuid>` (verified). Test compatibility with `--resume` + `--fork-session`.
- Codex: to be checked during implementation.
- Trait extension:
  ```rust
  enum SessionIdInjection {
      None,
      Supported,
      SupportedWithFork,  // can fork on resume
  }

  trait AgentProvider {
      fn session_id_injection(&self) -> SessionIdInjection { SessionIdInjection::None }
      fn build_launch_command(&self, opts: LaunchOpts) -> Option<String>;
  }

  struct LaunchOpts {
      pub session_id: Option<Uuid>,
      pub resume: Option<String>,
      pub fork_resume: bool,
      pub skip_permissions: bool,
      pub cwd: String,
  }
  ```
- `build_resume_command` deprecated in favor of `build_launch_command`.
- CodeTerm pre-generates UUID when provider supports injection, records it on pane **before** launch. No detection needed for fresh starts.

**Files:**
- `src-tauri/src/agent/provider.rs`
- `src-tauri/src/plugin/agent_claude/provider.rs`
- `src-tauri/src/plugin/agent_codex/provider.rs`
- `src-tauri/src/commands.rs` + `src-tauri/src/service/general_agent.rs` (call sites)

**Covers:** fresh launches, known-id resume, fork-resume. ~90% of the multi-session problem.

**Does not cover:** `/new` inside TUI (user-initiated session rotation).

#### B2 — Hooks for live tracking (secondary, covers /new)
Provider installs a per-agent hook that POSTs `{pane_id, session_id, event}` to CodeTerm REST API on session lifecycle events.

- REST: `POST /api/panes/:pane_id/session { session_id, source: "hook" }`
- Provider trait:
  ```rust
  fn install_session_hooks(&self, pane_id: &str, api_base_url: &str, token: &str) -> Result<(), String> { Ok(()) }
  fn uninstall_session_hooks(&self, pane_id: &str) -> Result<(), String> { Ok(()) }
  ```
- Claude hook: `~/.claude/hooks/codeterm_session_track.sh` — fires on `SessionStart` event. Reads `CLAUDE_SESSION_ID` + `CODETERM_PANE_ID` from env, `curl`s CodeTerm API.
- Need to verify:
  - Claude has `SessionStart` / `SessionChanged` hook events
  - Env propagation (agent exports session_id to hook)
- Env injection: CodeTerm already sets `CODETERM_PANE_ID` when spawning; pass api port + token too.

**Files:**
- `src-tauri/src/api/routes.rs` (new endpoint)
- `src-tauri/src/plugin/agent_claude/provider.rs` (`install_session_hooks`)
- New: hook script template in provider or `src-tauri/resources/`

**Covers:** `/new`, rotation, model switch triggering new session, etc.

**Limitations:** requires agent cooperation. Providers without hook system fall back to B3.

#### B3 — Content fingerprint fallback
Only when provider has neither `--session-id` nor hooks.

- Capture hash of first user-side stdin (pty write buffer already exists)
- After 500ms, scan session files in provider-specific session dir
- Match JSONL whose first user message contains the hash content
- Record matched id as `agent_session_id`

**Coverage:** provider-agnostic baseline. Loose (race, rare collisions on identical first prompts), but better than nothing.

**Files:**
- `src-tauri/src/agent/provider.rs` (`fn resolve_session_id(&self, ctx: ResolveCtx) -> Option<String>`)
- `src-tauri/src/agent/session_resolver.rs` (new — shared fingerprint logic)

#### B4 — Acceptance criteria for Task B
- Two Claude panes in the same cwd → each reports its correct `session_id`
- User runs `/new` → `pane.agent_session_id` updates within 2s
- Codex pane → detects session or gracefully falls back
- No more "ambiguous session" situations in logs

---

### Task C — Session search in CodeTerm

Port the concept from the community `session-search` skill (see references below) into CodeTerm as a first-class feature. **No vectors** — JSONLs are small + local, FTS5 is enough.

#### C1 — Index layer (provider-agnostic)
The community `search_sessions.py` skill only knows Claude's `~/.claude/projects/*.jsonl`. Our in-house version **must not** hardcode Claude. Every provider owns:
- Where session files live (Claude: `~/.claude/projects/<encoded-cwd>/`. Codex: `~/.codex/sessions/` or similar — audit needed. Aider/Goose/future: TBD.)
- How to parse each session file into messages (JSONL, rollout.json, plain text — format varies).

Split into two layers:

**1. Provider trait — enumeration + parsing:**
```rust
trait AgentProvider {
    fn enumerate_sessions(&self) -> Vec<SessionMetadata> { vec![] }
    fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> { vec![] }
}

struct SessionMetadata {
    pub session_id: String,
    pub provider: String,        // "claude", "codex", ...
    pub project_path: PathBuf,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub file_path: PathBuf,
}

struct SessionMessage {
    pub role: String,            // "user" | "assistant" | "system"
    pub text: String,
    pub timestamp: SystemTime,
    pub is_meta: bool,           // filter sidecar/meta messages
}
```

**2. Indexer — provider-agnostic:**
- Walks all registered providers via `agent_registry.all_providers()`
- Calls `enumerate_sessions()` per provider — gets flat list across ecosystem
- For each session file: mtime check vs index, re-parse via `parse_session_messages()` on change
- Writes rows to single table `session_messages` with `(session_id, provider, role, text, timestamp, ...)`
- FTS5 index on `text`

Result: "search sessions about X" hits Claude AND Codex AND anything else we add, no code changes to the search path. New provider = implement trait methods, done.

**Table design:**
```sql
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    project_path TEXT,
    title TEXT,
    summary TEXT,
    file_path TEXT NOT NULL,
    created INTEGER,
    modified INTEGER
);
CREATE TABLE session_messages (
    rowid INTEGER PRIMARY KEY,
    session_id TEXT REFERENCES sessions(session_id),
    role TEXT,
    text TEXT,
    timestamp INTEGER,
    is_meta INTEGER DEFAULT 0
);
CREATE VIRTUAL TABLE session_messages_fts USING fts5(text, content=session_messages, content_rowid=rowid);
```

Not reusing `mem_items` — session history can grow to 10k+ rows per active user, would dominate mem. Separate storage, shared FTS5 pattern.

#### C2 — Query API
- CLI: `codeterm sessions search "webpack config" [--project <path>] [--provider claude|codex|all] [--limit 10]`. `--provider all` is default.
- REST: `GET /api/sessions/search?q=...&project=...&provider=...&limit=...`
- Tauri command for frontend: `sessions_search(query, filters) -> Vec<SearchHit>`
- Result hit: session metadata + ranked snippets (FTS5 `snippet()`) + **provider badge** so UI shows whether hit is Claude / Codex / other.

#### C3 — UI integration
- Command palette entry: "Search past sessions…"
- Results panel in sidebar or modal
- Each hit shows: date, provider badge, project path, first matching snippets
- Actions: **Resume in current pane** (emits a `--resume` command) or **Open in new pane** (respects workspace cwd)

#### C4 — References
- Original skill concept: `/Users/rollacode/Downloads/SKILL 1.md`
- Reference search script: `/Users/rollacode/Downloads/search_sessions.py`

---

## Execution order

Ship as **one commit** covering A + B + C. No need to split — scope is cohesive and the changes touch the same provider trait.

Implement roughly in this order inside the single change:

1. **Task A** — validate `session_file_path` before resume; clear `agent_session_id` on miss. Pane-generic.
2. **Task B1** — add `build_launch_command(opts)` to `AgentProvider`, pre-generate UUID when provider supports `--session-id`, record on pane before launch. Keep `build_resume_command` as a thin wrapper during transition or delete it outright.
3. **Task B2** — REST endpoint for hook POSTs + Claude hook installer. Env-wire `CODETERM_PANE_ID` + api base/token into launched agent.
4. **Task B3** — content fingerprint fallback in `resolve_session_id` (default impl in trait, overridable).
5. **Task C** — session index (new SQLite table or reuse `mem_items`), FTS5 query, CLI + REST + frontend search UI.

Tests inline per CLAUDE.md. Run `./build.sh check` + `./build.sh test` + e2e spot-check before commit.

---

## Open questions

- Does `claude --session-id X` work together with `--resume Y`? Test before finalizing B1 design.
- Does Claude have a `SessionStart` (or similar) hook event name? Check `~/.claude/hooks/` docs or recent changelog.
- Should `sessions-index.json` be trusted (read Claude's own metadata) or should we rebuild it from JSONL scanning? Reading is cheaper; trusting their schema is fragile.
- For Codex: `~/.codex/sessions/` layout and resume mechanics — audit during PR2.
- Sessions from OTHER machines synced via mesh — do we index them too? (probably yes for search, no for resume since provider can't reach them).

---

## Non-goals

- Cross-agent session migration (resume a Claude session under Codex, etc.) — out of scope.
- Vector/semantic search over session history — defer. FTS5 is sufficient for "find that session about X".
- UI-level session manager / session tree view — nice to have, not part of this plan.

---

## Appendix: Codex on-disk schema (audit for C implementation)

Needed to plug Codex into `enumerate_sessions` + `parse_session_messages` the same way Claude is plugged in today.

### File layout
```
~/.codex/sessions/
  <YYYY>/<MM>/<DD>/
    rollout-<YYYY-MM-DDTHH-MM-SS>-<uuid>.jsonl
```

- Date-partitioned three directories deep (not flat like Claude).
- Filename format: `rollout-{iso-timestamp-with-hyphens}-{uuid}.jsonl`. The timestamp is `YYYY-MM-DDTHH-MM-SS` (19 chars, hyphen-delimited, not colon). The UUID is UUID v7 (time-ordered).
- `src-tauri/src/plugin/agent_codex/provider.rs::extract_session_uuid` already strips the prefix and returns the uuid. `collect_jsonl_files` already recurses the date tree. Reuse both for `enumerate_sessions`.
- Codex CLI resumes via `codex resume <uuid>` — NOT the filename stem.

### JSONL line schema
Every line is a JSON object with the same envelope:
```json
{
  "timestamp": "2026-04-10T11:50:27.862Z",
  "type": "session_meta | event_msg | response_item | turn_context",
  "payload": { ... }
}
```

Line types seen in practice:
| `type`           | Frequency      | Contents                                                                   |
|------------------|----------------|----------------------------------------------------------------------------|
| `session_meta`   | 1 per file     | id, cwd, model_provider, cli_version, source ("exec"/"interactive"), base_instructions |
| `turn_context`   | 1 per file     | per-turn model + policy settings                                           |
| `event_msg`      | many           | lifecycle events — `task_started`, approval results, etc.                  |
| `response_item`  | majority       | actual conversation payload (messages, function calls, reasoning)          |

### `session_meta` → SessionMetadata mapping
```
session_id   = payload.id            (UUID v7)
project_path = payload.cwd           (absolute path, no encoding)
created_ts   = payload.timestamp     (parsed as RFC3339)
modified_ts  = file mtime
title        = — not provided by codex (leave None)
summary      = — same
```
Note: unlike Claude, Codex does NOT keep a `sessions-index.json` beside the JSONLs. Titles/summaries aren't available without reading the file content.

### `response_item` → SessionMessage mapping
Only rows with `payload.type == "message"` count as conversation:
```json
{
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "user" | "assistant" | "developer",
    "content": [
      { "type": "input_text" | "output_text" | "text", "text": "..." },
      ...
    ]
  }
}
```
Extraction:
- `role` → copied verbatim into `SessionMessage.role`.
- `text` → concatenate every content block whose `type` is in {`input_text`, `output_text`, `text`}.
- `ts`  → parse top-level `timestamp`.

Other `payload.type` values to skip for search: `function_call`, `function_call_output`, `reasoning`. (`reasoning` blocks might be worth indexing later for "find where Codex thought about X" but they add noise; skip for v1.)

### Meta/noise filtering
Codex does not carry an `isMeta` flag like Claude. The codebase injects several system-style messages as `role: user` for the agent's own consumption — they should set `is_meta=true` so `search_sessions` skips them:

- User messages whose first content block starts with any of:
  - `# AGENTS.md instructions`
  - `<INSTRUCTIONS>`
  - `<environment_context>`
  - `You are a Graph Worker` (CodeTerm's own bootstrap prompt)
  - `<permissions instructions>` (this one appears with `role: developer`, already skipped by role filter)

- Skip `role: developer` entirely — it's always system-prompt content.

Keep the filter list short and explicit; don't regex-guess. Unknown system injections will show up as hits until we add them — that's acceptable.

### Implementation sketch
```rust
// In ClaudeProvider pattern, adapted for Codex:

fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
    let Some(root) = Self::sessions_root() else { return Vec::new(); };
    let mut out = Vec::new();
    // walk YYYY/MM/DD/*.jsonl — reuse collect_jsonl_files with no exclude/recency filter
    // for each file: read FIRST LINE ONLY to extract session_meta.payload.id + payload.cwd
    //                extract_session_uuid(stem) as fallback if first line not session_meta
    //                file metadata for timestamps
    // return SessionMetadata { provider: "codex", ... }
    out
}

fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
    // stream lines, filter type == "response_item", payload.type == "message"
    // skip role == "developer"
    // extract text from input_text/output_text/text blocks
    // mark is_meta=true when text starts with known injection markers
}
```

### Open items before shipping Codex search
1. Confirm Codex supports `--session-id <uuid>` equivalent (earlier `codex --help` audit showed no flag). If still not, B1 stays Claude-only; Codex relies on post-launch detection by UUID v7 time-order + cwd match.
2. Decide whether to index `reasoning` blocks. Current plan: no. Revisit if users complain "can't find where Codex worked through X".
3. Decide session-file-path strategy for `session_exists`: Codex's `session_file_path` already calls `find_session_file_by_uuid` which scans the tree — it's O(n) per check, acceptable for Task A validation (rare) but not for hot-path checks. If we ever need hot-path lookups, cache date→uuid map.
