# Facet 6: Testing And Fixtures

## Goal

Make PTY agent-state detection fast to improve and hard to regress.

State classifiers live in provider plugins. Core owns only:

- `PaneStateSnapshot` construction from the pane's own screen/session.
- Generic `PaneStateClassification` deserialization.
- Reactor safety gates.
- Shared fixture replay utilities.

Provider tests own:

- Provider-specific raw PTY captures.
- Golden and negative expectations.
- Matcher and ensemble behavior.

## Corpus Layout

Use one shared corpus root:

```text
tests/fixtures/pane-state/
  claude/
    rate_limited/
      session-limit-wait-default/
        capture.json
        chunks.bin
        expected.json
        screen.txt
      session-limit-upgrade-highlighted/
        ...
    clarifying_question/
    waiting_on_auth/
    negative/
      wait-word-in-prose/
      wait-option-while-working/
      paused-tui-no-prompt/
      normal-tool-output/
  codex/
    update_prompt/
    waiting_on_auth/
    negative/
  opencode/
    ...
  cursor/
    ...
```

Keep existing provider-local fixtures while AS6 lands, then move them into the
shared corpus and leave provider test helpers reading from the shared root.

## Fixture Files

`capture.json`:

```json
{
  "schema": 1,
  "provider": "claude",
  "state": "rate_limited",
  "name": "session-limit-wait-default",
  "source": {
    "capturedAt": "2026-06-20T10:15:00Z",
    "codetermBuild": "git-sha",
    "providerVersion": "claude-code 1.2.3",
    "platform": "macos",
    "terminal": { "cols": 120, "rows": 50 }
  },
  "snapshot": {
    "agentType": "claude",
    "sessionId": "redacted-or-null",
    "cwd": "/repo",
    "tailRows": 60
  },
  "chunks": [
    { "offset": 0, "len": 418, "dtMs": 0 },
    { "offset": 418, "len": 91, "dtMs": 37 }
  ],
  "redactions": [
    { "kind": "path", "value": "/Users/andrey", "replacement": "/Users/example" }
  ],
  "notes": "Real rate-limit menu. Wait option is already highlighted."
}
```

`chunks.bin`:

- Raw PTY bytes concatenated in read order.
- Includes ANSI, cursor movement, alternate-screen writes, CRLF, and partial UTF-8 boundaries.
- Never normalize before writing.

`screen.txt`:

- Deterministic rendered output from `ScreenState::tail_text(pty_id, 60)` after replaying all chunks.
- Used for review diffs and JS-only tests.
- Generated, not the source of truth.

`expected.json`:

```json
{
  "classification": {
    "state": "rate_limited",
    "minConfidence": 0.8,
    "data": {
      "resetAt": { "kind": "rfc3339", "required": true }
    },
    "action": {
      "keys": ["enter"],
      "delayedResume": {
        "text": "continue your work",
        "thenKeys": ["enter"],
        "afterDataResetAt": true
      }
    }
  },
  "negative": false
}
```

Negative fixture expectation:

```json
{
  "classification": {
    "forbiddenStates": [
      "rate_limited",
      "clarifying_question",
      "waiting_on_auth",
      "update_prompt"
    ],
    "maxConfidenceForAction": 0.79,
    "forbidActionKeys": true,
    "forbidDelayedResume": true
  },
  "negative": true
}
```

## Capture Workflow

Add a dev-only CLI command:

```bash
codeterm pane capture-state-fixture \
  --pane PANE_ID \
  --provider claude \
  --state rate_limited \
  --name session-limit-wait-default \
  --out tests/fixtures/pane-state/claude/rate_limited/session-limit-wait-default
```

Capture source:

- Read raw bytes from `OutputBuffer::get_full(pty_id)` plus absolute offsets.
- Read rendered screen from `ScreenState::tail_text(pty_id, 60)`.
- Read pane metadata from mux: provider id, session id, cwd, size.
- Read CodeTerm build from `identify` source, not HTTP.

Capture rules:

- Capture once from a real pane after the state is visibly stable.
- Redact secrets and home paths at capture time.
- Store redaction metadata.
- Do not hand-edit `chunks.bin`.
- Regenerate `screen.txt` after redaction by replaying `chunks.bin`.
- A fixture is accepted only if replayed screen equals `screen.txt`.

For transient mid-stream negatives, capture while the agent is still printing:

```bash
codeterm pane capture-state-fixture \
  --pane PANE_ID \
  --provider claude \
  --state negative \
  --name wait-option-while-working \
  --cut latest-2s
```

`--cut latest-2s` keeps only chunks whose read time falls inside the last two
seconds. This creates partial-screen fixtures that reproduce false positives
caused by incomplete TUI redraws.

## Replay

Replay is deterministic and timing-independent by default:

1. Create a fresh `ScreenState`.
2. Feed every chunk from `chunks.bin` in recorded order through `ScreenState::process`.
3. Ignore `dtMs` for classification tests.
4. Build `PaneStateSnapshot` from rendered `tail_text`.
5. Call the provider plugin `classifyPaneState`.
6. Assert `expected.json`.

Timing is retained only for future streaming-state tests. Classifiers must not
depend on wall-clock chunk timing except through injected `host.unixNowMs()`.
Tests pin `unixNowMs()`.

Optional replay modes:

- `full`: feed all chunks.
- `prefix`: feed every chunk prefix and assert negatives do not become actionable before the final stable frame.
- `chunked`: feed one chunk at a time and classify after each chunk.
- `crlf`: normalize rendered `screen.txt` to CRLF and assert identical classification.

`prefix` is required for all positive fixtures with key actions.

## Fast Test Harness

Provide one TypeScript runner:

```bash
npx tsx tests/pane-state/run-corpus.ts --provider claude
npx tsx tests/pane-state/run-corpus.ts --provider codex --watch
npx tsx tests/pane-state/run-corpus.ts --changed
```

Runner behavior:

- Loads the built provider `plugin.js` in a QuickJS-like VM host stub.
- Discovers fixtures under `tests/fixtures/pane-state/<provider>`.
- Replays raw bytes through a small VT100 renderer when available.
- Falls back to `screen.txt` only for JS-only local loops.
- Pins `host.unixNowMs()` from fixture config or runner flag.
- Prints one line per failing fixture: provider, fixture id, actual state, confidence, action.
- Exits in under one second for the normal corpus.

Rust replay test:

```bash
cargo test pane_state_corpus --lib
```

Rust behavior:

- Uses real `ScreenState`.
- Uses real QuickJS plugin runtime.
- Verifies JS wire output deserializes into native `PaneStateClassification`.
- Verifies core `decide` rejects all negative fixtures and below-threshold positives.

Provider unit tests stay narrow:

- Matcher tests: one function, synthetic screen strings, no fixture IO.
- Ensemble tests: weak signal stays below threshold.
- Corpus tests: real captures only.

## Golden Corpus

Each provider must have at least one positive fixture per supported state:

- `waiting_on_auth`: no `keys`, no `delayedResume`, surface allowed.
- `update_prompt`: exact navigation to safe option, never blind Enter on install/update.
- `rate_limited`: reset parse when present, safe wait navigation, delayed resume only when reset time is reliable.
- `clarifying_question`: extracted question text, surface only, no keys.
- `idle_stuck`: no key action until a separate policy explicitly permits one.

Golden assertions:

- `state` equals expected.
- `confidence >= minConfidence`.
- `data` fields match exact values when deterministic.
- Time fields are RFC3339 and relative constraints hold.
- `action.keys` matches exactly.
- No extra action fields appear unless expected.

## Negative Corpus

The negative corpus is the release gate. A negative may return `working`,
`unknown`, or `null`; it must not return an actionable state.

Required negatives per provider:

- Normal assistant prose containing words like `wait`, `limit`, `reset`, `update`, `question`, `sign in`.
- Mid-stream output with busy footer: `esc to interrupt`, `ctrl+c to cancel`, `Working...`.
- Tool output containing numbered lists.
- Diffs and package-manager update notices.
- Shell prompts after command completion.
- Paused TUI frames that contain a menu from earlier scrollback but no current prompt footer.
- Partial redraw prefixes of every positive fixture.

Specific regression fixture for Andrey's wait false positive:

```text
I will wait for the build output before editing the launcher.

  1. Start with provider fixtures
  2. Update the reactor tests

Working...
```

Expected:

- Not `rate_limited`.
- Not `clarifying_question`.
- No `keys`.
- Confidence below `GLOBAL_THRESHOLD` if any classifier sees a weak signal.

Any production false positive becomes a fixture before the fix. The fix is not
accepted until the fixture fails before the heuristic change and passes after it.

## Native And JS Parity

Parity has two levels:

1. Boundary parity: JS `classifyPaneState` output round-trips into native
   `PaneStateClassification` for every action shape.
2. Corpus parity: bundled provider plugin produces the same verdict under the
   TypeScript runner and Rust QuickJS runtime for every shared fixture.

For providers with native fallback logic, add a native implementation corpus
runner and assert native equals JS on shared fixtures. For JS-only providers,
the Rust parity assertion is deserialization plus reactor decision parity.

## CI Gates

Fast local gate:

```bash
npm run test:pane-state
```

CI gate:

```bash
./build.sh check
npm run test:pane-state
cargo test pane_state_corpus --lib
```

`npm run test:pane-state` should:

- Build affected provider bundles.
- Run corpus tests for changed providers.
- Always run all negative fixtures.

CI fails when:

- A negative fixture returns action keys.
- A negative fixture returns delayed resume.
- A negative fixture reaches `GLOBAL_THRESHOLD` for an actionable state.
- A positive fixture drops below `minConfidence`.
- Fixture replay does not reproduce `screen.txt`.
- Fixture schema validation fails.

## Iteration Loop

Recommended loop while tuning a provider:

```bash
npm run build:plugin -- provider-claude
npx tsx tests/pane-state/run-corpus.ts --provider claude --watch --focus wait
```

The watch runner should print:

```text
FAIL claude/negative/wait-word-in-prose
  expected: no actionable state
  actual:   rate_limited confidence=0.82 keys=["enter"]
  screen:   tests/fixtures/pane-state/claude/negative/wait-word-in-prose/screen.txt
```

Do not require a daemon, Tauri, browser, or live provider account for the fast
loop. Live capture is only for adding fixtures.

## Risks

- Raw captures may contain secrets. Redaction is mandatory before writing fixtures.
- ANSI replay may diverge across renderer versions. Keep `screen.txt` generated
  and fail loudly when renderer output changes.
- Timing-dependent classifiers become flaky. Inject time and ignore chunk timing
  in normal classification tests.
- Fixture bloat slows the loop. Keep `chunks.bin` capped to the relevant screen
  episode unless a full-session regression needs more history.
- Provider UI copy changes. Prefer structural fixtures and multiple examples
  over exact phrase dependence.

## First Implementation Slice

1. Move existing Codex and Claude text fixtures into shared corpus folders.
2. Add `expected.json` next to each fixture.
3. Add the TypeScript corpus runner using `screen.txt`.
4. Add Rust corpus replay for `ScreenState` and QuickJS deserialization.
5. Add raw-byte capture command.
6. Replace synthetic text-only positives with captured raw fixtures as real
   panes become available.

<!-- revision: 1 -->
