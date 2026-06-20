# Snapshotter v2 — proper corpus building, lifecycle coverage, snapshot validation

> Owner: Dante, from Andrey 2026-06-20, after live testing the v1 bench. v1 proved the
> ISOLATED RUNNER works (thin harness, no parser dup, parse-until-perfect). v1's CORPUS is
> the weak half: snapshotter is claude-only + manual, coverage is happy-path pickers only,
> no lifecycle stages, no snapshot-quality checks, real on-disk sessions unused. This plan
> fixes the corpus side. Branch `plugins-rethink`. Builds on `testbench/` + the runner.

## What live testing exposed (the gaps)
- **cursor / opencode multi-step multi-select wizards DON'T parse.** Both providers render rich
  wizards live (cursor: "Миграция A/B/C — этап 1 из 3", "Question 1 of 4", `[ ] … Space select`,
  `←/→ question`; opencode similar). The v1 workers only captured STATIC pickers (model/rules/
  skills lists) — they never drove the providers into these wizards, so the parsers were never
  exercised on them. The library has **zero** multi-step/multi-select wizard snapshots.
- **opencode DOES render multi-select wizards** (live screenshot: "A: Audit scope … select all
  that apply", `[ ] Monolith → Microservices`, `tab select / enter toggle / esc dismiss`). Its
  model (deepseek) sometimes emits invalid JSON for the structured question (`JSON Parse error`),
  but the TUI wizard still renders — so we parse it FROM THE SCREEN, not the JSON. This is a real
  opencode shape we must cover.
- **codex multi-section: skip (Andrey's call).** codex doesn't build these multi-section wizards
  reliably, so we deliberately don't parse codex multi-section. Keep codex single pickers
  (model/approval/options) which are solid. (No screenshot evidence — this is a product decision.)
- **No lifecycle coverage.** Every snapshot is "picker is on screen". Nothing for: fresh pane /
  no messages, mid-conversation, after a completed turn, error/limit states. The region-anchoring
  false-positive (parser scraped the welcome banner) slipped through precisely because there was
  no "startup, no picker → expect null" snapshot.
- **Real sessions unused.** 577 claude + 115 codex session files sit on disk — a free, real,
  lifecycle-rich corpus for the transcript/ordering parsers — and the bench reads none of them.
- **Snapshot quality unverified.** No check that a `screen.txt` is a clean settled frame (not a
  mid-animation/spinner frame), that a `raw.cast` re-renders to its `screen.txt`, or that an
  `expected.json` field actually appears in the input. A bad snapshot silently makes a green or
  red case meaningless.

## THE CORE RETHINK: parse the ATTRIBUTED grid, not flattened glyphs
Live byte-level investigation of our own `raw.cast` files proved the current parsers are
fundamentally lossy — they regex the FLATTENED text (`1.` / `A)` / `❯`) and discard the
structure the TUI actually emits.

Evidence (decoded from real casts):
- **cursor model-picker**: the selected row is marked ONLY by a true-color BACKGROUND
  (`ESC[48;2;21;21;21m`) — NO marker glyph, no number. In flattened text the selected option is
  indistinguishable from the others. (This is why cursor send-back guessed the highlight.)
- **claude options**: selection = `❯` glyph + accent FG (`ESC[38;2;215;119;87m`); plus `?25h/?25l`
  (cursor hide/show) toggled 113× during redraws, box-drawing for the banner.
- Each provider marks selection DIFFERENTLY and STRUCTURALLY (cursor=bg color, claude=glyph+fg,
  opencode=checkboxes). There is no universal "number/letter" to regex.

Proven feasibility: `vt100::Cell` (the crate we already parse with) exposes `fgcolor()`,
`bgcolor()`, `inverse()`, `bold()`, `underline()`, `contents()`. We call NONE of them —
`SignalExtractor` flattens via `contents_between` into `Vec<String>`. The rich data is one
accessor away; we throw it out.

The rethink — a tiered detector keyed on attributes + position, NOT glyphs:
- **Tier 1 — attribute-aware structural detection (deterministic; ~90% of the win).** Expose an
  ATTRIBUTED screen from vt100 (per-cell fg/bg/inverse/bold + cursor pos + DECSET state). Detect
  the **selected option** as the attribute discontinuity — the contiguous run whose bg differs
  from the dominant bg, OR `inverse`, OR a distinct accent fg. Detect the **option block** as the
  maximal run of rows with parallel structure (same indent/styling) adjacent to the selection,
  bounded by the footer/box/cursor. Detect the **question** as the styled line just above the
  block. Multi-select = checkbox glyphs/toggles + "select all"/footer hints. One provider-agnostic
  structural detector replaces N per-provider glyph regexes; selection state comes from the
  attribute that the renderer actually used.
- **Tier 2 — structural anchors (partly exist via `picker_active`).** DECSET (alt-screen ?1049,
  hidden cursor ?25l, bracketed-paste ?2004), a footer-hint lexicon ("Enter to select", "Space
  select", "Tab toggle", "←/→ question", "esc dismiss"), box-drawing frames — as gates + region
  bounds + to reject banners/normal text.
- **Tier 3 — ML, later (optional, corpus-fed).** The snapshot library becomes labeled training
  data (attributed screen → question/options/selected). A small per-row sequence labeler
  (is-question / is-option / is-selected / is-chrome) over attribute features (bg-delta, inverse,
  indent, leading-glyph, fg-accent) — CRF/logistic, the classic terminal-scraping approach; an
  LLM is overkill for a hot path. Build Tier 1 first; reach for Tier 3 only if it plateaus.

Prior art to review: tmux `capture-pane -e` (preserves SGR), pyte/vt100 attribute grids, TUI test
frameworks + screen readers that detect focus via inverse/bg. The field's standard answer is
attribute+position heuristics, not ML — which is exactly Tier 1.

Snapshotter implication (answers "is our snapshotter bad?"): half-bad. It DOES keep `raw.cast`
(bytes preserved), but (a) the runner feeds parsers the flattened `screen.txt` → attributes lost;
(b) many snapshots have NO `raw.cast` at all; (c) there's no attributed artifact to review. v2 must
ALWAYS capture `raw.cast`, derive + feed the ATTRIBUTED grid to parsers, and store a reviewable
attributed dump (e.g. `screen.ansi` via vt100 `contents_formatted`, or `cells.json`).

## Key architecture insight: TWO snapshot sources
Different parsers consume different inputs; the corpus must reflect that.

- **Source A — live PTY drive (terminal/screen).** The ONLY way to get picker/wizard SCREENS and
  agent-state streams: drive a real agent over the PTY, capture `raw.cast` + rendered `screen.txt`.
  Feeds `provider_screen` (parsePrompt/classify) and `agent_state_replay`. This is what the
  snapshotter automates. Lifecycle here = drive the agent into each state.
- **Source B — real on-disk session files (transcript).** Picker screens are NOT in session
  files (they're ephemeral terminal UI) — but the CHAT TRANSCRIPT is. claude `~/.claude/projects/
  */*.jsonl`, codex `~/.codex/sessions/*.jsonl`, cursor `~/.cursor/chats/<md5>/<uuid>/`. These
  feed `parseSessionDelta` / chat normalization / **ordering**. 692 real sessions = abundant real
  lifecycle (empty, mid-convo, tool-heavy, multi-turn) for free, no driving needed.

v1 only has Source A (claude-only). v2 adds: generalized Source A + a new Source B + validation.

## v2 components

### 1. Generalized snapshotter (replace the claude-only demo)
`testbench/capture.sh --provider <p> --situation <s> --prompt "…" --until <matcher>`:
- provider-agnostic spawn + capture via `dc` (never rebuilds dev).
- a **shape catalog** `testbench/shapes/<provider>.json`: per shape, the drive prompt + the
  "settled" matcher (regex on the screen that means the picker finished rendering). Covers each
  provider's real shapes incl. the multi-step/multi-select wizards.
- **version stamping**: meta.json records the provider CLI version (`claude --version`,
  `codex --version`, cursor/opencode equivalents) — required so the nightly drift runner can
  attribute a parse change to a provider version bump.
- **lifecycle situations** are first-class: `fresh-no-messages`, `after-one-turn`,
  `picker-active`, `multi-step-wizard`, `multi-select`, `error-limit`, …

### 2. Session-sourced snapshots (new, Source B)
`testbench/ingest-session.sh --provider <p> [--session <id|path>] --stage <stage>`:
- pull a real on-disk session, slice it to a lifecycle stage (empty / first-turn / mid /
  tool-call / long), write a transcript snapshot + expected normalized output.
- a new runner unit `session_transcript` that replays the session bytes through the REAL
  `parseSessionDelta` + chat normalization + ordering (mergeById) in-process, asserts the
  normalized messages + their ORDER. This finally gives the ordering fix a real-data offline gate
  (today only `merge.test.ts` synthetic cases + a load-contaminated live eval).
- privacy: ingest scrubs/echoes only what the snapshot needs; keep snapshots reviewable.

### 3. Snapshot doctor (quality/validation) — `testbench/doctor.sh`
Flags unusable snapshots so a green/red case actually means something:
- **cast↔screen consistency**: re-render `raw.cast` through the same vt100 the runner uses;
  assert the final frame equals `screen.txt`. Catches stale/hand-edited screens.
- **settled-frame check**: reject `screen.txt` that is a mid-animation frame (braille spinner
  chars, cursor mid-line, "Thinking…/Esc to interrupt" with no options) for picker cases.
- **oracle-grounding**: every literal string in `expected.json` (question/option labels) must
  actually appear in the input. Catches oracles that drifted from the snapshot.
- **schema/parse**: `raw.cast` rows parse; `expected.json` matches the case schema.
- doctor runs in CI alongside `run.sh`; a broken snapshot fails loudly, not silently.

### 4. Capture → propose-expected (semi-auto oracle)
The snapshotter writes `proposed-expected.json` = the CURRENT parser's actual output on the fresh
capture. Human diffs and promotes to `expected.json`. Makes oracle authoring fast + reviewable
and is the core loop for the nightly drift runner (capture latest → propose → diff vs frozen →
flag drift).

## Coverage to add (concrete, after the tooling)
- **cursor multi-step multi-select wizard** (live: "Миграция A/B/C — этап 1 из 3", "Question 1
  of 4", `[ ] … Space select`, `←/→ question`) → parse into sections + multi-select + per-question
  options + send-back; gating snapshot.
- **opencode multi-select wizard** (live: "A: Audit scope … select all that apply", checkbox rows,
  `tab select / enter toggle`) → parse from the SCREEN (ignore the model's invalid JSON) into
  sections + multi-select + options + send-back; gating snapshot.
- **claude lifecycle**: `startup-no-prompt` (null) + `banner-with-active-picker` (in flight in
  task/claudefix) + `after-turn-no-prompt`.
- **session_transcript** stages for claude + codex + cursor (ordering + normalization real-data gate).
- **codex multi-section**: explicitly OUT (invalid JSON, unreliable) — record the decision in
  the shape catalog so it's a deliberate skip, not an oversight.
- **orphan cleanup**: the 16 `screen.txt`-only dirs (migrated pane-state fixtures) get an
  `expected.json` (so they actually run) or are removed.

## Phased execution
- **P1 — validation + version stamping**: doctor.sh + provider-version in meta.json + run doctor
  over the existing 22 cases (find out NOW if v1 snapshots are bad). Cheapest, highest-signal —
  answers Andrey's "may the snapshots themselves be bad?" first.
- **P2 — generalized snapshotter**: capture.sh + shape catalog + propose-expected; re-capture the
  existing shapes through it (uniform meta + version).
- **P3 — coverage**: cursor + opencode multi-step/multi-select wizards (drive → snapshot →
  parse-until-perfect → card) + claude lifecycle.
- **P4 — session-sourced**: ingest-session.sh + session_transcript unit over real claude/codex/
  cursor sessions (real-data ordering/normalization gate).
- **P5 — nightly drift runner**: capture latest per provider → propose-expected → diff vs frozen
  → flag drift + open fix task. (The original vision; everything above is its prerequisite.)

## Open question for the claudefix worker
task/claudefix (pane 7618154f) is hands-on in the bench right now. When it reports, fold in its
firsthand observations on snapshot ergonomics (is screen.txt easy to author against? is the messy
vt100 grid usable?) before locking P2/P3 shapes.

## Spike results (proven live) + execution plan (parallel on codex)
Spike (`src-tauri/tests/attr_spike.rs`, vt100 grid over real `raw.cast`):
- The attributed-grid mechanic WORKS — we read per-cell `bgcolor/fgcolor/inverse` correctly
  (cursor composer bar bg `rgb(21,21,21)`+inverse; claude banner accent fg `rgb(215,119,87)`;
  claude input bg `rgb(55,55,55)`).
- Selection encoding is HETEROGENEOUS: cursor `/model` selected row = `→` GLYPH (list rows have
  default bg/inverse); claude = `❯`+accent fg; opencode = checkboxes; composer bars = bg; some
  TUIs = inverse. So the detector must FUSE signals (glyph + inverse + bg≠dominant + accent-fg +
  structural row-grouping + footer anchor), not rely on attributes alone OR glyphs alone.
- Snapshot quality is the real blocker: old `cursor/model-picker` cast captured the WRONG frame
  (picker already closed); `claude/options-abc` is claude PROSE (A/B/C in a message), not a TUI
  picker. Capturing the picker-ACTIVE frame is essential (proven: re-capturing cursor `/model`
  and stopping while open yields the real list).

### Execution — Phase 1 (parallel codex workers, independent NOW)
- **W1 — universal attributed-grid detector (in the bench, no prod contract change yet).** Expose
  an `AttributedScreen` from vt100 (per-cell fg/bg/inverse/bold + cursor + dominant-bg); build a
  FUSION picker detector {question, options, selected, multi_select} over glyph+attr+structure+
  footer; prove accuracy on re-captured REAL pickers vs the dumb text parser. Bench-only first to
  de-risk the provider-boundary contract.
- **W2 — lossless + right-frame snapshotter + doctor.** `capture.rs` byte-exact (base64 non-UTF8);
  `capture.sh` stops at the picker-ACTIVE frame; `doctor.sh` flags bad snapshots (prose-not-picker,
  no selection signal, wrong frame); run doctor over the existing 22 and report which are bad. Then
  re-capture REAL active-frame pickers (cursor /model + wizard, claude wizard, opencode
  permission/question) to feed W1.
- **W3 — opencode SSE handle (gold-standard non-scraping path).** Spawn opencode with fixed
  `--port`, subscribe `/event` for `permission.asked`/`question.asked` (options included), respond
  via `POST /session/:id/permissions/:id`. Capability-gated (`supports_structured_prompts`), PTY
  stays for display. Proves the Tier-1 handle for the one provider that has it in PTY mode.

### Execution — Phase 2 (after W1 lands, parallel per-provider)
Wire the fusion detector into the prod plugin contract (native+JS parity), then per-provider
parse-until-perfect on re-captured real pickers: cursor (/model + multi-step multi-select wizard),
claude (real pickers + AskUserQuestion wizard, drop literal-string banner stripping for structural
banner rejection), codex (model/approval single pickers; multi-section skip). Then nightly drift.

Orchestrator stays integrator (re-gate + merge each). W1/W2/W3 are file-disjoint enough to run
concurrently; W1 consumes W2's re-captured snapshots as they land.

<!-- revision: 2 -->
