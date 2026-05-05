# Per-pane token tracking — v1.3.0 plan

Goal: every pane in CodeTerm shows live token + cost stats for the agent
running in it. Status dot in the pane chrome expands on hover, opens a
detailed popup on click. Settings → Usage section aggregates across all
panes by day / week / month / provider / model.

Three providers (Claude Code, Codex, OpenCode) all log token usage to
local files we can read. We don't proxy the API — we just read what's
already on disk and aggregate it.

Reference points (public projects, used for architecture, not code):

- **ccusage** (`ryoppippi/ccusage`) — reads Claude Code JSONL, supports
  Codex via a sibling package. Validates that the approach works and
  surfaces the correct pricing tables. We do not bundle ccusage; we
  re-implement in Rust with our own schema.
- **cc-statusline** (`chongdashu/cc-statusline`) — proves the statusline
  surface integration. We don't reuse the statusline (we have our own
  pane chrome) but the data it consumes is identical.

## Where the numbers come from

| Provider | Log path (default) | Format | Fields |
|----------|--------------------|--------|--------|
| Claude Code | `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` | JSONL, one event per line | `usage.input_tokens`, `usage.output_tokens`, `usage.cache_creation_input_tokens`, `usage.cache_read_input_tokens`, `model`, `id`, `timestamp` |
| Codex CLI | `~/.codex/sessions/<session-id>.jsonl` | JSONL | `prompt_tokens`, `completion_tokens`, `cached_tokens`, `model`, `timestamp` |
| OpenCode | `~/.local/share/opencode/storage/session/<projectID>/<sessionID>.json` (+ `~/.local/share/opencode/storage/message/...`) | JSON sessions, JSON-per-message events | `tokens.input`, `tokens.output`, `tokens.reasoning`, `cost`, `modelID`, `providerID`, `time` |

CodeTerm already tracks `agent_session_id` per pane (Pane.agent_session_id),
so the join from a session-log file back to a pane is trivial:
file path + last 100 lines → session_id → pane lookup → record the delta.

## Storage

New tables in the existing knowledge.db (or a sibling `usage.db` — see
"Open questions"):

```sql
CREATE TABLE token_event (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,                  -- unix ms
  pane_id TEXT,                         -- nullable: events from sessions
                                        -- not bound to a known pane
  agent_session_id TEXT NOT NULL,
  agent_provider TEXT NOT NULL,         -- 'claude' | 'codex' | 'opencode'
  model TEXT NOT NULL,                  -- e.g. 'sonnet', 'gpt-5.4', 'anthropic/claude-opus-4-7'
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
  cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  reasoning_tokens INTEGER NOT NULL DEFAULT 0,   -- codex / opencode-routed
  cost_usd REAL NOT NULL DEFAULT 0,     -- computed at ingest time
  source_file TEXT,                     -- audit trail
  source_line_offset INTEGER            -- de-dup key with source_file
);

CREATE INDEX idx_token_event_pane     ON token_event(pane_id, ts);
CREATE INDEX idx_token_event_session  ON token_event(agent_session_id, ts);
CREATE INDEX idx_token_event_dedup    ON token_event(source_file, source_line_offset);

-- Materialised aggregate refreshed by the watcher on each ingest.
CREATE TABLE token_session_total (
  agent_session_id TEXT PRIMARY KEY,
  pane_id TEXT,
  agent_provider TEXT NOT NULL,
  first_ts INTEGER NOT NULL,
  last_ts INTEGER NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cache_creation_tokens INTEGER NOT NULL,
  cache_read_tokens INTEGER NOT NULL,
  reasoning_tokens INTEGER NOT NULL,
  cost_usd REAL NOT NULL
);
```

`source_file + source_line_offset` lets the watcher resume safely after
a crash without double-counting.

## Backend

### Watcher

- New module `src-tauri/src/usage/watcher.rs`. One `tokio::spawn` per
  provider, polling its log directory at 5 s intervals (provider-tunable).
- File mtime + size cached in memory; re-read only the tail when size grew.
- Each provider has its own parser (`usage/parsers/{claude,codex,opencode}.rs`)
  that converts a raw line/event into a normalised `TokenEvent`.
- After parse: compute cost via `usage/pricing.rs`, insert into
  `token_event`, upsert into `token_session_total`, emit
  `pane-token-usage` Tauri event.

### Pricing table

`src-tauri/src/usage/pricing.rs` ships a `PRICES: HashMap<&str, ModelPricing>`
hardcoded with current Anthropic / OpenAI / OpenCode-via-router prices.
Schema: `(model, input_per_mtok, output_per_mtok, cache_write_per_mtok,
cache_read_per_mtok)`. Refreshed each release.

A nightly fetch-from-vendor-pricing-API is out of scope; manual table
update on every CodeTerm release is fine. Stale pricing logged when a
model name has no entry — cost stays zero, tokens still tracked.

### API

New routes in `src-tauri/src/api/usage_routes.rs`:

- `GET /api/usage/pane/{pane_id}` — current session totals + last 50
  events for the popup chart.
- `GET /api/usage/aggregate?since=<ts>&group_by=<day|week|month|provider|model>`
  — for the Settings → Usage panel.
- `GET /api/usage/live/{pane_id}` — SSE stream of new events for that
  pane, used by the status dot to animate on each new write.

### Live event

When watcher inserts a row, emit Tauri event `pane-token-usage` with
`{pane_id, session_total: {input, output, cache_creation, cache_read,
cost_usd}, last_event: {...}}`. Frontend store reduces it into pane
state.

## Frontend

### Status dot in pane chrome

Lives in the existing pane title bar. Initial render: a 6 × 6 colored
dot. Color encodes spend tier:

- Grey: < $0.10
- Green: $0.10–$1
- Amber: $1–$5
- Red: > $5

On hover, the dot expands into a horizontal chip:

```
↑12.3k  ↓4.5k  $0.08    (cache 78%)
```

Numbers are k/M-rounded; cost is the live session total. Cache % is
`cache_read_tokens / (input_tokens + cache_read_tokens)`.

### Click → popup

Anchored to the dot, ~360 × 220. Sections:

1. **Header.** Provider · model · session id (truncated · click to copy).
2. **Sparkline** of input + output tokens per turn for last ~20 turns.
3. **Numbers row.** Total in / out / cache write / cache read / cost.
4. **Cache hit %** with explanation tooltip.
5. **Compare row.** "This pane vs your last 7d avg per pane" — quick
   gut-check whether this is a heavy session.

### Settings → Usage section

New tab next to "Mesh". Renders:

- Top row: total spend today / this week / this month, side by side.
- Stacked bar chart: daily totals, last 30 days, color by provider.
- Table: top 10 sessions by cost, filterable by provider / model. Each
  row links to the pane (if still open) or to a session-resume action.
- Settings: "Pricing table version: 2026-05-01 — refresh on update"
  (read-only info row).

## Phases

### Phase 1 — Storage + Claude parser + watcher

- Migration that creates the two tables.
- `usage::parsers::claude` reads ~/.claude/projects/**/*.jsonl tail.
- `usage::watcher` polls Claude path every 5 s. Inserts events.
- `usage::pricing` initial table for current Claude models.
- Tests: golden JSONL fixtures piped through parser, expected
  `TokenEvent` structs.

Acceptance: open a Claude pane, run a turn, watcher records the event
within 6 s, totals visible via `/api/usage/pane/{id}`.

### Phase 2 — Codex + OpenCode parsers

- `usage::parsers::codex` and `usage::parsers::opencode`.
- Pricing tables for both.
- Same fixture style.

Acceptance: opening a Codex pane and an OpenCode pane both produce rows
in `token_event` after one turn each.

### Phase 3 — Status dot + hover chip

- `src/components/PaneChrome/UsageDot.tsx` (new).
- Subscribes to `pane-token-usage` Tauri event.
- Reads initial state via `/api/usage/pane/{id}`.
- Pure CSS hover expansion, no JS animation.

Acceptance: dot renders in every pane chrome, updates within ~1 s of a
new event, hover chip reads the right numbers.

### Phase 4 — Click popup

- `src/components/PaneChrome/UsagePopup.tsx`.
- Sparkline via existing chart util (or thin SVG; we're already lean on
  deps).
- Compare row queries `/api/usage/aggregate?group_by=pane&since=…`.

Acceptance: click on dot in any pane opens the popup, all sections
render with correct data, popup follows the dot when scrolled.

### Phase 5 — Settings → Usage tab

- `src/components/Settings/UsageTab.tsx`.
- Three aggregate tiles + one stacked bar chart + one table.

Acceptance: settings panel shows total spend matching the sum of all
session totals; bar chart reproduces the same data grouped by day.

### Phase 6 — Polish

- Color tier thresholds configurable (some users won't care below $1,
  others below $0.01).
- Export CSV from settings panel.
- "Cost since pane open" vs "cost lifetime of the agent session" toggle
  in popup.

## Sequencing

Phase 1 + 2 (backend) ship in one PR — tables + three parsers + watcher
+ pricing table + tests. ~3 days.

Phase 3 + 4 (per-pane UI) ship in a second PR. ~2 days.

Phase 5 (settings aggregate) is a third PR. ~1 day.

Phase 6 ships incrementally as feedback comes in.

Total ~6 days of focused work. Independent of the v1.2.0 token-tools
work; can ship in parallel.

## Risks

- **Pricing drift.** Vendors change list prices without warning. Cost
  rows go stale. Mitigation: pricing table version-stamped in DB, banner
  in Settings → Usage when version is older than 30 days.
- **JSONL format drift.** Each provider could rev their log format.
  Mitigation: parser modules tolerate unknown fields (serde
  `#[serde(default)]`), only fail on missing required fields. Drop
  unparseable lines, log warning, continue.
- **OpenCode session id format.** OpenCode uses `ses_*`-prefixed ids
  inside a per-project directory; CodeTerm currently stores the bare id
  in `Pane.agent_session_id`. May need a join helper that strips the
  prefix.
- **Watcher overhead on huge JSONL.** A daily-active session can hit
  multi-MB. Mitigation: tail-only reads using `source_file +
  source_line_offset` dedupe. Initial scan caps at last 1000 lines per
  file.
- **Privacy.** Token totals leak nothing; cost reveals model + spend.
  Local-only by default, never synced over mesh.

## Open questions

- Same DB as `mem::knowledge.db` or sibling `usage.db`? Lean: same DB
  (single file backup), separate schema. Mem already has WAL set up;
  reusing avoids two checkpoint hooks.
- Per-machine rollup or mesh-aware "team usage"? Probably v1.4.0+. For
  now, per-machine only.
- Codex stream events may arrive after `Stop` — do we backfill the
  pane_id when a row arrives for a session whose pane has already
  closed? Yes; `pane_id` is recorded at session creation and stays
  bound for the session's lifetime.
- Color tiers: hard-coded vs config? Lean: hard-coded for v1.3.0,
  exposed in Settings later.
