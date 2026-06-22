# Detection / Regression Improvement Loop (R1–R20)

Worker under coordinator pane `70670bb8`. Branch `plugins-rethink`, baseline
HEAD `3e76cf0d`. Architectural fixes only; every claimed improvement carries a
test/metric/script-output for visibility. No commits/pushes (coordinator owns
that). Auto-approve / auto-respond is **out of scope** — not inspected or
changed.

---

## Round 1 — Parity coverage for the provider control surface (+ 2 real bugs)

**Finding (highest value):** AGENTS.md plugin rule #2 requires every
AgentProvider trait method a JS drop-in can implement to have a parity case
proving native and JS agree. Four boundary methods had **no** parity coverage:
`parse_prompt`, `stop_keystroke`, `model_launch_args`,
`needs_structured_prompt_server` (only `parse_interactive_events` was covered).

**Decision:** fix-now. Wrote the missing parity tests first (TDD). They
surfaced two genuine native↔JS divergences:

1. **`stop_keystroke` divergence (live UI bug).** The bundled claude
   `plugin.json` declares `stopKey: ""` (Esc), but native `ClaudeProvider`
   never overrode `stop_keystroke()` → returned `None` (trait default).
   `supports_stop` in the provider descriptor is derived from
   `stop_keystroke().is_some()` (provider.rs:1070), so the native-backed path
   would hide the Stop affordance while the JS path shows it. Claude's TUI
   interrupts on Esc (the trait doc and `screen_has_tui`'s "esc to interrupt"
   both confirm), so JS was correct and native was the bug.

2. **`parse_prompt` footer pollution (live Chat/Web UI bug).** The claude
   plugin's `parseNumberedOptions` / `parseCheckboxOptions` captured option
   labels across line boundaries. Any footer on a following line could be
   absorbed by the **last option**:
   `"3. No\n\nEsc to cancel · Tab to amend"` → label `"No Esc to cancel · Tab to amend"`.
   The native line-scoped parser never had this problem. Users saw a polluted
   final option label in the permission UI.

**Changes:**
- `src-tauri/src/plugin/js_runtime/parity_test/claude/commands.rs` (new) —
  6 parity tests for the four methods (positive, unknown-footer, and negative
  `parse_prompt`).
- `src-tauri/src/plugin/js_runtime/parity_test/claude/mod.rs` — register
  `mod commands;`.
- `src-tauri/src/plugin/agent_claude/provider/mod.rs` — override
  `stop_keystroke()` → `Some("\u{1b}")`, matching the manifest and TUI reality.
- `plugins/provider-claude/src/plugin.ts` — make numbered and checkbox option
  parsing line-scoped, matching the native parser and avoiding footer-copy
  dependency.
- `plugins/provider-claude/plugin.js` — regenerated bundle (`node
  scripts/build-plugin.mjs provider-claude`); generated output, src↔bundle in
  sync.

**Commands & outcomes:**
```
node scripts/build-plugin.mjs provider-claude
  → built provider-claude logic → plugins/provider-claude/plugin.js (48.5 KB)

cargo test --lib plugin::js_runtime::parity_test::claude::commands
  → BEFORE fix: 3 passed, 2 failed (stop_keystroke + parse_prompt divergence)
  → AFTER fix:  6 passed

bash scripts/plugin-tests.sh provider-claude
  → 53 passed, 0 failed ; ✓ all plugin test suites green

cargo test --lib plugin::js_runtime::parity_test
  → 65 passed (full parity suite, no regressions from mod registration)
```

**Bundle gate note:** `node scripts/check-plugin-bundles.mjs` reports the
bundle "stale" only because the rebuilt `plugins/provider-claude/plugin.js`
differs from HEAD (uncommitted). Source↔bundle are consistent; the
`SYSTEM_PLUGIN_HASHES` table is regenerated at build time by `build.rs` from
the on-disk bundle, so the Rust build stays self-consistent. Committing is the
coordinator's call. No bundle-gate / hash-table source files were touched.

**Status:** complete, green.

---

## Round 2 — Remove provider-id hardcode from usage-probe readiness oracle

**Finding:** `src-tauri/src/usage/probe.rs` gated `/usage` panel readiness with
`has_parseable_markers(text, provider)`, a `match provider { "claude" => …,
"codex" => … }` substring check. That is a capability leak: whether the
captured screen is parseable is provider-specific knowledge that already lives
behind the `AgentProvider::usage_parser()` capability. A new provider shipping
a usage parser would silently never be recognized as "ready".

**Decision:** fix-now (clean, capability-based, testable).

**Change (`src-tauri/src/usage/probe.rs`):**
- Replace `has_parseable_markers` with `output_is_parseable(svc, provider,
  text)` → resolves the provider via the registry and asks its OWN
  `usage_parser()` whether `parse(text)` yields a snapshot with `has_any_field()`.
- Extract the pure oracle `parser_accepts(Option<&dyn UsageParser>, text)` so
  it's unit-testable without a live registry.
- No provider-id literal remains in the readiness path.

**Tests (`usage::probe::tests`, new):**
- `parser_accepts_none_is_not_ready` — no parser → not ready (preserves the old
  "unknown provider → false" semantics).
- `parser_accepts_tracks_field_presence` — stub parser: field present → ready,
  error/empty → not ready.
- `real_claude_parser_gates_like_old_markers` — the real `ClaudeTuiUsageParser`
  accepts the captured `fixtures/usage_v2_1.txt` screen and rejects a loading
  screen, proving no regression vs the old `"% used" && "Resets"` markers.

**Commands & outcomes:**
```
cargo test --lib usage::probe::tests          → 3 passed
cargo test --lib plugin::js_runtime::parity_test::claude → 37 passed
grep has_parseable_markers src-tauri/src       → no dangling refs
```

**Considered & rejected this round:** `update/shutdown.rs` provider-id fallback
(`"claude" | "codex" => "/exit"`) is already capability-first
(`provider.exit_command()` then a *documented* compile-time fallback for
stub/loading-order cases) with its own parity test — churning it adds risk
without architectural gain. Left as-is.

**Status:** complete, green.

---

## Round 3 — Non-vacuous chat-preprocessor ctx boundary tests

**Finding:** `PreprocessCtx` (`src-tauri/src/chatpreprocessor/mod.rs`) carries
`pane_id`, `cwd`, `provider`, `turn_index`, but every existing test built the
ctx with `..Default::default()` — so the host→JS serialization of
`cwd`/`provider`/`turn_index` and the snake_case→camelCase rename
(`turn_index`→`turnIndex`) were **never exercised**. A field drop or wrong key
would silently corrupt what plugins read.

**Decision:** fix-now (pure test addition, no production change).

**Tests added (`plugin::js_runtime::chatpreprocessor::tests`):**
- `ctx_fields_cross_the_boundary_camelcase` — plugin echoes
  `provider|cwd|turnIndex|paneId|text`; asserts all four populated fields
  arrive intact under their camelCase keys.
- `optional_ctx_fields_serialize_as_null` — `None` cwd/provider reach JS as
  `null` (preserving plugin truthiness checks); `turn_index` default is `0`.

**Commands & outcomes:**
```
cargo test --lib plugin::js_runtime::chatpreprocessor → 10 passed (was 8)
```

**Status:** complete, green.

---

## Round 4 — File-size guard: first safe split + plan for the rest

**Finding:** `scripts/check-file-sizes.sh` was red on 3 non-allowlisted files
(`offline_state_classifier.rs` 1370, `model_catalog.rs` 1357, `pane_routes.rs`
1073). Pre-existing debt, but the guard is CI-wired, so it's a real red signal.

**Decision:** fix-now the one safe single-step split; plan the rest (the other
two need production-code responsibility splits that would churn files the
coordinator is actively editing — collision risk outweighs the gain right now).

**Change — `model_catalog.rs` 1357 → 791 (off the red list):**
Per AGENTS.md ("keep `mod tests;` in source, move the body to
`<name>/tests.rs`"), moved the 567-line inline `#[cfg(test)] mod tests` body to
`src-tauri/src/agent/model_catalog/tests.rs`; source now declares
`#[cfg(test)] mod tests;`. Zero production-logic change. The test module was
self-contained (`use super::*;` only — verified no `#[path]`/`include!`/nested
mods), so `super` still resolves to the same module.

```
cargo test --lib agent::model_catalog → 20 passed (identical set, post-split)
bash scripts/check-file-sizes.sh      → model_catalog.rs no longer listed
                                         (2 files remain red, down from 3)
```

**Split plan for the remaining two (deferred — not implemented):**
- `offline_state_classifier.rs` (1370; 284 test lines, ~1086 production):
  moving tests out alone leaves ~1087 (still red), so it needs a production
  split too. Plan: `offline_state_classifier/tests.rs` + extract the per-state
  sub-classifiers (rate-limit / auth / clarifying-question) into a
  `classifiers` submodule, leaving the dispatcher + shared types in the facade.
- `pane_routes.rs` (1073; only 4 test lines, ~1069 production): split by route
  group into a `pane_routes/` module — `lifecycle.rs` (list/create/close/
  rename/resize/focus/describe), `io.rs` (send_text/get_pane_output/
  get_pane_text/stream_pane_output/shell_command_snapshot), `exec.rs`
  (exec_in_pane/agent_run_log/start_capture/stop_capture/get_pane_usage/
  stop_pane). `mod.rs` keeps the router wiring as a thin facade (<800 lines).

**Also evaluated, no edit:**
- `hook_routes.rs` `"claude"` defaults (lines 35, 294) — these are
  Claude-only hook endpoints (`has_hooks()` is Claude-exclusive), so defaulting
  the agent type to `claude` is contextually correct; the `TODO(plugin-prep)`
  already records the right future architecture (plugins owning HTTP routes).
- "snapshot restore fallback provider-id hardcode" — searched `session/`;
  no provider-id literal exists there. Restore is already capability-clean.

**Status:** model_catalog split complete + green; remaining splits planned.

---

## Round 5 — Parity for the in-pane command surface

**Finding:** `compact_command`, `exit_command`, `model_switch_format` are
overridden in native Claude AND read from the manifest in JS — the identical
manifest/native drift class that hid the R1 `stop_keystroke` bug — yet had no
parity test. A silent divergence here would, e.g., send a stale `/exit` at
graceful shutdown on one path only.

**Decision:** fix-now (test-only).

**Test added** (`...parity_test::claude::commands::in_pane_command_surface_native_js_parity`):
asserts native == JS for all three, and anchors the literal values
(`/compact`, `/exit`, `/model {model}`) so even a both-sides edit trips review.
(Distinct from the auto-approve/auto-respond policy, which is out of scope.)

```
cargo test --lib ...parity_test::claude::commands → 7 passed (was 6)
```

**Status:** complete, green.

---

## Round 6 — Transition-latency diagnostic for the agent-state replay eval

**Finding:** `tests/agent_state_replay.rs` hard-gates curated seeds on
final-state correctness + no-forbidden-mid-stream-state, but measures **no
transition latency** — there was no data on how long the classifier takes to
settle on the truth state. The priority item ("turn transition-latency
diagnostics into a hard gate if practical") can't responsibly start with a
gate: there are no per-fixture latency budgets, so a gate would be vacuous
(loose) or flaky (tight). The honest first step is the diagnostic.

**Decision:** fix-now the diagnostic (MEASURED, never fatal); explicitly defer
the gate until the numbers justify a budget.

**Change (`tests/agent_state_replay.rs`):**
- `StepResult` gains `now_ms` (replay clock per step).
- `convergence_ms(steps, truth)` — replay-ms (relative to first step) at which
  the classifier first reaches `truth` and never leaves; `None` if it never
  settles (already a golden-oracle failure).
- Scorecard gains a `conv_ms` per-fixture column + a
  `p50/max over settled fixtures` summary. Pure diagnostic — does not affect
  the gating decision.

**Verified non-vacuous (`cargo test --test agent_state_replay -- --nocapture`):**
```
fixture                       bucket truth          predicted      fW  fTC  conv_ms
codex/update_prompt           seed   update_prompt  update_prompt   0    0      520
codex/working_streaming       seed   working        working         0    0        0
...
convergence latency over 2 settled fixture(s): p50=520ms max=520ms
```
(REAL baseline fixtures show `—` = never settled on truth — the existing,
correctly non-gating baseline findings.)

**Gate decision (deferred, with rationale):** once more curated seeds carry
real captures, set a per-state budget (e.g. gate `update_prompt` at
≤ ~1.5× observed p50) and promote `convergence_ms > budget` to a gating
failure for `gating:true` fixtures only. Not done now — 2 settled seeds is too
thin a sample to fix a threshold without risking flakiness.

**Status:** diagnostic complete + verified; gate deferred by design.

---

## Round 7 — L4 opencode `--session` Windows quoting (verified already-fixed)

**Assigned:** make opencode's `--session` quoting platform-aware
(`host.shell.quote` is always POSIX; `quoteFor(s, platform)` is PowerShell on
Windows), consistent with sibling args.

**State on arrival:** the fix was already applied in the working tree (by the
coordinator / a parallel worker) — `plugins/provider-opencode/src/plugin.ts`
lines 144 + 156 now use `host.shell.quoteFor(String(p.sessionId),
host.platform())`; `tests/plugin.test.ts` gained a non-vacuous Windows case
(stub host returns `BAD(...)` for the wrong `quote` and `Q(platform:...)` for
`quoteFor`, so using the non-platform path would fail the assertion); bundle
rebuilt. No duplicate edit made — verified instead.

**Verification:**
```
bash scripts/plugin-tests.sh all → all suites green (0 failed); the two new
  ✓ buildLaunchCommand quotes --session with quoteFor(platform)
  ✓ buildResumeCommand quotes --session with quoteFor(platform)
cargo test --lib parity_test::opencode → 6 passed
bundle plugin.js --session lines use quoteFor (src↔bundle in sync)
```

**Status:** complete (fix present + verified green); no change authored by me.

---

## Round 8 — Generalize L4: codex + cursor `--resume` Windows quoting

**Finding:** the same POSIX-only quoting bug L4 fixed in opencode existed,
unfixed, in the two sibling providers — `provider-codex/src/plugin.ts`
`buildResumeCommand` quoted the session UUID with `host.shell.quote(...)`
(line 125) and `provider-cursor/src/plugin.ts` did the same for `--resume`
(line 119), while both quote their launch args with `quoteFor(..., platform)`.
On Windows the resume command would be POSIX-quoted, inconsistent with launch.

**Decision:** fix-now — same architectural fix, low collision (neither plugin
is in the coordinator's active edit set), exactly the class the coordinator
endorsed.

**Changes:**
- `provider-codex/src/plugin.ts`: `quote(uuid)` → `quoteFor(uuid, host.platform())`.
- `provider-cursor/src/plugin.ts`: `quote(sessionId)` → `quoteFor(sessionId, host.platform())`.
- Both bundles rebuilt (`build-plugin.mjs`).
- Non-vacuous Windows test in each `tests/plugin.test.ts` (stub host returns
  `BAD(...)` for `quote`, `Q(platform:...)` for `quoteFor`, so a regression to
  the POSIX path fails the assertion).

**Verification:**
```
bash scripts/plugin-tests.sh all → all suites green; the two new
  ✓ buildResumeCommand quotes session id with quoteFor(platform)   (codex)
  ✓ buildResumeCommand quotes session id with quoteFor(platform)   (cursor)
cargo test --lib parity_test::codex  → 7 passed
cargo test --lib parity_test::cursor → 9 passed
npm run check:versions → synced 1.4.6
```

**Completeness sweep** (closing the class, not just instances):
```
for f in plugins/*/src/*.ts; do awk '/shell\.quote\(/' ; done → (empty)
```
No non-platform-aware `shell.quote(` remains in any plugin source. All four PTY
providers (claude/codex/cursor/opencode) now quote session ids platform-aware.

**Follow-up:** a lint/grep guard (`host.shell.quote(` should not appear in a
launch/resume command builder) would prevent regressions — noted for the
coordinator as it touches shared CI scripting.

**Status:** complete, green.

---

## Round 9 — startup chat-UI provider allowlist (analysis + small honest fix)

**Question (coordinator):** does `init_states` have a safe capability source for
the default-chat-UI provider allowlist before the dynamic registry exists?

**Answer: partially — and the part that matters can't be made static.**
- `AgentProvider::supports_chat_ui()` is a RUNTIME capability derived from a
  loaded plugin VM (`has_js("sessionJsonlPath") || uses_structured_chat() ||
  native()…`), not a manifest flag. It cannot be computed without loading the
  provider.
- Ordering (lib.rs): `init_states()` @96 runs *before* the `AgentRegistry` is
  created @346, before embedded plugins are bootstrapped to disk @436, and
  before providers are loaded/registered @466–541. So there is no provider to
  query at `init_states` time. The existing comment is accurate.
- ⇒ the chat-*capability* decision must remain a hardcoded bootstrap allowlist.
  Did NOT force a capability patch (none is sound here).

**Small honest fix made** (the one static source that IS available — the
embedded plugin set):
- The 4-id list was duplicated (an inline array in `init_states` + a `matches!`
  in `bundled_agent_provider_supports_chat_ui`). Hoisted to a single
  `const BUNDLED_CHAT_UI_PROVIDERS`, documented as bootstrap debt with the
  reason. Behavior identical.
- Added a drift-guard test `chat_ui_allowlist_entries_are_all_embedded_plugins`:
  every allowlist id must be in `plugin::embedded::ids()` (statically embedded,
  available pre-registry). Catches the allowlist naming a provider removed from
  the build. Correct invariant: allowlist ⊆ embedded (it's a chat-capable
  *subset*, so equality would be wrong).

```
cargo test --lib startup::states → 4 passed
```
(While writing the test I confirmed `embedded::ids()` returns bare provider ids
`["claude","codex","cursor","opencode"]`, not `provider-<id>` dir names.)

**Proposed proper architecture (deferred — not a tiny change):** move chat-view
defaulting out of `init_states` into a post-registry startup hook that runs
after @541 and queries the real `agent_registry` for each pane's provider
`supports_chat_ui()`. That removes the bootstrap allowlist entirely and makes
restored/general-agent panes follow the same capability path as fresh spawns.
It touches the mux-state lifecycle (the chat-view default currently mutates
`mux_state_inner` before it's wrapped in `Arc`) and general-agent workspace
creation timing, so it's out of scope for this tiny item.

**Status:** analysis complete; small DRY + drift-guard fix landed green;
capability deferral proposed.

---

## Round 10 — Provider-boundary parity for codex/cursor/opencode (JS-only)

**Constraint:** codex/cursor/opencode have NO native sibling, so native↔JS
parity is impossible. Honest "parity" here = bundled-plugin output vs the
documented capability contract, on shared fixtures; only drift found by a test
gets fixed.

**Tests added (`plugin::js_runtime::parity_test`, 3 new, all green):**
- `structured_prompt_server_capability_contract` — opencode is the ONLY bundled
  provider with `needs_structured_prompt_server()==true` (host-bound SSE);
  claude/codex/cursor are false; and `needs_…==true ⟹ supports_structured_prompts()`
  (trait-documented). Guards against a manifest binding the wrong transport.
- `model_launch_args_match_each_cli_flag_format` — pins the REAL CLI flag per
  provider (codex/opencode `-m`, claude/cursor `--model`) + empty-model→no flag.
  A flag flip would launch with an arg the CLI rejects.
- `supports_stop_matches_stop_keystroke` — the descriptor's `supports_stop` must
  equal `stop_keystroke().is_some()` for all four (two reads of one capability;
  a split = a Stop button that sends nothing, or a hidden working one). This is
  the R1 drift class, guarded on the JS path.

```
cargo test --lib plugin::js_runtime::parity_test → 70 passed (was 67)
```

**Finding reported, NOT patched (can't verify honestly):** the `stop_keystroke`
trait doc says "cursor-agent listens for Ctrl+C (\x03)", but
`provider-cursor/plugin.json` declares `stopKey: "\x1b"` (Esc). If cursor-agent
only interrupts on Ctrl+C, the Stop button sends the wrong byte. I did NOT
change it — verifying cursor-agent's real interrupt key needs a live CLI, not a
doc comment. Flagged for the coordinator. (The test above asserts the
descriptor *invariant*, deliberately not the raw byte, so it neither entrenches
nor hides the discrepancy.)

**Honestly not addable (reported, not forced):**
- `parse_prompt` for codex/cursor/opencode is already covered by each provider's
  JS contract test suite (7 / 13 / 15 assertions respectively) against real TUI
  fixtures; with no native to diff, a Rust-side copy would only duplicate them.
- `parse_interactive_events` is a Claude-only structured (JSONL AskUserQuestion)
  capability — already has native↔JS parity (claude/sessions.rs); the PTY-menu
  providers don't implement it.

**Status:** complete, green; 1 finding flagged for live verification.

---

## Round 10 — JS-provider capability contract parity

**Finding:** codex/cursor/opencode are JS-only providers, so native<->JS parity
is not meaningful for several methods. The honest test shape is
plugin-output-vs-documented contract.

**Tests added (`plugin::js_runtime::parity_test`):**
- `structured_prompt_server_capability_contract` — opencode is the only bundled
  provider that needs a host-bound structured-prompt server, and
  `needs_structured_prompt_server() => supports_structured_prompts()`.
- `model_launch_args_match_each_cli_flag_format` — pins real CLI model flags:
  claude/cursor `--model`, codex/opencode `-m`; empty model produces no flags.
- `supports_stop_matches_stop_keystroke` — descriptor `supports_stop` stays in
  lockstep with `stop_keystroke().is_some()` for all bundled providers.

**Finding reported, NOT patched:** cursor's trait doc says Ctrl+C, while its
manifest declares Esc as `stopKey`. That needs live cursor-agent verification,
not a blind patch.

```
cargo test --lib plugin::js_runtime::parity_test → 70 passed
```

**Status:** complete, green.

---

## Commit-readiness split — file-size blocker removed

Adversary correctly found that `file-sizes.yml` runs
`scripts/check-file-sizes.sh --fail`, so the WIP could not land while
`offline_state_classifier.rs` and `pane_routes.rs` were over 1000 lines.

**Changes:**
- `offline_state_classifier.rs` split into:
  - `offline_state_classifier.rs` — 792 lines, state machine + shared helpers.
  - `offline_state_classifier/status.rs` — status/completion extraction.
  - `offline_state_classifier/tests.rs` — moved inline tests.
- `pane_routes.rs` split into:
  - `pane_routes.rs` — 766 lines, pane route facade and non-send handlers.
  - `pane_routes/send.rs` — send DTOs/actions and `/api/panes/{id}/send`.

**Verification:**
```
bash scripts/check-file-sizes.sh --fail → green
cargo test --lib agent::offline_state_classifier → 14 passed
cargo test --lib tests_send_text_body          → 21 passed
```

**Status:** commit blocker removed.

---

## Round 11 — active_turn_footer copy-fragility: analysis + regression gate

**Task:** reduce literal-vendor-copy dependence in `active_turn_footer` only if a
structural, replay-backed improvement is safe; else report why + propose a gate.

**Read-only map:** `active_turn_footer` (offline_state_classifier.rs:703) is LIVE
— `classify_state`/`extract_status_line` are called from `service/output.rs:456,
474` (the live PTY status path). It keys off literal interrupt copy
(`"esc to interrupt"`, `"ctrl+c to stop"`) combined with structural shapes
(`⏵⏵` prefix, progress-meter glyphs, elapsed `(3m 54s)` timer). It had **zero**
unit/replay coverage, and the replay eval (`agent_state_replay.rs`) exercises the
JS plugin classifier, not this native path.

**Finding — a pure-structural replacement is NOT safe (do not remove the copy):**
the literal interrupt hint is the load-bearing semantic signal. Every structural
shape it pairs with also appears on idle/completed screens:
- `⏵⏵` is also the IDLE footer marker (`⏵⏵ bypass permissions on (shift+tab to cycle)`),
- a progress-meter shape / elapsed `(3m 54s)` timer also render in a COMPLETED
  turn summary.
So the interrupt hint is the only thing shown *only while a turn is active*;
dropping it for structure-only matching would mis-read idle/done as "working" —
exactly the broad classifier churn this task forbids.

**Instead implemented the proposed gate** (test-only, no classifier change):
`offline_state_classifier/tests.rs` gains two tests that pin the contract and
make copy-drift LOUD instead of silent —
- `active_turn_footer_fires_on_real_active_footers` (claude ⏵⏵+esc, esc+timer,
  esc+meter; codex →+ctrl+c),
- `active_turn_footer_silent_on_idle_and_completed_screens` (idle `⏵⏵` footer,
  completed-turn timer, meter-glyphs-alone, plain shell — all must be false).

The negatives empirically prove the analysis: idle `⏵⏵` / completed timer /
meter-glyphs all return false **because** they lack the literal hint.

```
cargo test --lib agent::offline_state_classifier        → 16 passed (+2)
bash scripts/check-file-sizes.sh                        → ✅ green (tests.rs 331)
```

**Status:** analysis complete; structural removal rejected with evidence; gate
landed green; file-size stays green.

---

## Consolidated verification (after R10 + post-split)

```
cargo test --lib                          → 2053 passed
cargo test --test agent_state_replay      → 1 passed
npx tsc --noEmit                          → green
bash scripts/plugin-tests.sh all          → 455 passed, 0 failed
npm run check:versions                    → synced to 1.4.6
./build.sh check                          → clean (3 pre-existing warnings, not mine)
bash scripts/check-file-sizes.sh --fail   → green
```

## Status after R10

10 substantive rounds; real bugs found and fixed include Claude stop-key drift,
Claude prompt footer pollution, usage-probe provider-id/copy hardcoding,
chatBackend preprocessor staging asymmetry, and POSIX-only session quoting in
provider command builders. All current landing blockers are removed; remaining
items are follow-up work, not commit blockers:
- Latency hard-gate — needs more curated seed fixtures before a non-flaky
  budget can be set (R6); diagnostic now in place to gather the data.
- Playwright ChatView coverage — needs a running dev instance (operational
  dependency outside this read/edit loop).

## Priority queue (remaining)
- Remove/replace provider-id hardcodes in core where safe (usage/probe.rs,
  startup/states.rs, hook_routes defaults, snapshot restore fallback).
- Further route/module splits for readability only; file-size CI is green.
- Transition-latency diagnostics → hard gate (if practical).
- Playwright ChatView smoke/golden coverage where local/ROG exposed gaps.
- chat-preprocessor ctx/provider/cwd + preset UI/backend non-vacuous tests.

<!-- revision: 1 -->
