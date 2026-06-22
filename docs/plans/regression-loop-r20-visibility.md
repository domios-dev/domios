# Regression-loop R20 — visibility runner log

Standing measurement log for the 20-round improvement loop on branch
`plugins-rethink`. Worker role: Detection/Regression Visibility Runner under
coordinator pane `70670bb8`. Goal: keep every round measurable — run the gates,
capture before/after on any Lead diff, and name the blind spots.

Source of truth for gate semantics: `docs/plans/detection-visibility-after-fix-runbook.md`.
All replay numbers come from the SAME production classifier the live path uses
(`render_timeline.rs` → `offline_state_classifier`); Step 0 regen is mandatory
after any classifier edit.

## Environment at R20 entry

- HEAD: `3e76cf0d` ("feat: add chat preprocessor plugin").
- Dev daemon (`:7686`) build: `a0da8919` — **one commit behind HEAD** (the
  behind-by-one is exactly the chat-preprocessor commit).
- Implication: offline replay gates rebuild `render_timeline` from current source
  → valid regardless of daemon. **LIVE checks (stop-oracle, chat-order --live)
  would run a STALE daemon → NOT trusted until dev is rebuilt to HEAD.**
- Prod `:7685` untouched. No auto-approve / auto-respond policy work.

## Established baseline (post-push green, carried in from prior rounds)

| Gate | Command | Result |
|---|---|---|
| Rust check | `./build.sh check` | green |
| Version sync | `npm run check:versions` | green |
| Plugin TS tests | `bash scripts/plugin-tests.sh all` | green |
| TS typecheck | `npx tsc --noEmit` | green |
| TS unit | `bash scripts/ts-unit-tests.sh` | green |
| Rust full | `./build.sh test` | **2037 passed** |
| Playwright smoke | ChatView smoke | 1 passed |
| Golden ChatView (local) | — | SKIPS when `VITE_CODETERM_TEST` hook absent |
| Golden ChatView (ROG WebView2) | remote `plugins-rethink` e5bffa2b | passed (dev started w/ `VITE_CODETERM_TEST=1`) |
| File sizes | `scripts/check-file-sizes.sh` | **RED** — `offline_state_classifier.rs`, `model_catalog.rs`, `pane_routes.rs` over limit |

## R20 current WIP sweep

| Gate | Command | Result |
|---|---|---|
| Rust full | `./build.sh test` | **2053 passed, 0 failed** |
| Rust check | `./build.sh check` | green; only pre-existing `clipboard_files.rs` warnings |
| TS typecheck | `npx tsc --noEmit` | green |
| Plugin TS tests | `bash scripts/plugin-tests.sh all` | **455 passed, 0 failed** |
| chatBackend staging | `npx tsx packages/chat-ui/src/MessageBubble.providers.test.tsx` | **13 passed** (exec/tool_call/file cards, structured + mesh inbound) |
| Provider parity | `cargo test parity_test::{codex,cursor,opencode}` + Claude command parity | green |
| Agent-state replay | `cargo test --test agent_state_replay -- --nocapture` | green; convergence diagnostic prints p50=520ms, max=520ms over settled seeds |
| File sizes | `bash scripts/check-file-sizes.sh --fail` | **GREEN** — `model_catalog.rs`, `offline_state_classifier.rs`, and `pane_routes.rs` split below the hard limit |
| Bundle drift gate | `pnpm check:plugin-bundles` | expected red before commit; generated provider bundles differ from HEAD by design — confirmed stale `plugin.js` in `provider-cursor` (`cursor-agent --resume` + `quoteFor`) and `provider-opencode` (`--session` → `quoteFor(.., host.platform())`). Source `.ts` changed, bundles not yet rebuilt. TS plugin tests (run via `tsx` on source) and offline replay gates (Rust `render_timeline`) are unaffected; only the shipped JS bundle lags until `pnpm build:plugin` + commit. |

## R20 fresh measurement (HEAD 3e76cf0d, regen of 50 replays)

Regen: 50/50 casts → JSON, 0 failures. Corpus = 14 claude / 12 codex / 12 cursor /
10 opencode / 2 synthetic.

### Offline replay gates

**Post-split re-confirmation:** after `offline_state_classifier.rs` was split into
`offline_state_classifier/`, render_timeline was rebuilt + all 50 replays regenerated,
and all three gates below produced **identical results** to pre-split (transition-latency
GREEN with the same per-provider numbers, strobe 154, permission 1/121 frame-0 only).
The classifier directory-split is behavior-neutral.

| Gate | Command | Result | Notes |
|---|---|---|---|
| Transition-latency (HARD GATE) | `python3 testbench/transition-latency.py <regen> --gate` | **GREEN** | All providers 0 safety-net. claude 86, codex 55, cursor 44, opencode **26** transitions — all via structural composer anchor. opencode safety-net **0** (was 4/19 at R9). opencode `sem_med=1537ms` (latency, not a fail). |
| Strobe metric (tracking, not a hard gate) | `python3 testbench/strobe-metric.py <regen> --glob '*.json'` | **154 strobes (RED)** | Dominated by long-chat 0ms flips: opencode systems_design 41 (question↔working — the known open item), claude systems_design 47, claude snake-canonical 28. Mostly 0ms transient flips inside long replays. Threshold `WORKING_GRACE_MS=1500`. |
| Permission false-idle guard | `bash testbench/permission-guard-check.sh` | **SUBSTANTIVE PASS** (script prints PARTIAL, exit 1) | 1/121 idle = **frame 0 only** (pre-paint blank screen, row0/col0 — correctly idle). All 120 frames with the rendered box read `question`; final=`question`. Not the bug (bug = 121/121 idle). Script's strict exit trips on the benign seed frame. |
| Rust replay detection | `cargo test --test agent_state_replay` | **PASS** (1 test, 3.01s) | Single `#[test]` but `read_dir`-discovers the provider/state fixture corpus and asserts per-entry through BOTH core SignalExtractor and provider classifier (9 asserts). Non-vacuous. |

### Plugin / preprocessor smoke (non-mutating, `npx tsx`)

| Suite | Passed |
|---|---|
| provider-claude detect | 32 |
| provider-claude status | 11 |
| provider-claude plugin | 32 |
| provider-claude classify-pane-state | 53 |
| provider-codex plugin | 45 |
| provider-codex provider-helper-parity | 17 |
| provider-codex classify-pane-state | 27 |
| provider-opencode plugin | 47 |
| provider-cursor plugin | 125 |
| **chat-preprocessor plugin** | **11** |
| src/lib/plugins registry | 25 |
| chat-ui recognize | 31 |
| **TOTAL** | **455 passed, 0 failed** |

chat-preprocessor coverage (non-vacuous): stable meta + text slot, disabled/missing-preset
passthrough, template-preset render, mem-recall replacement (block prepended, multiple hits,
`memK` respected, original text preserved), empty/error mem passthrough.

### Channel / version consistency

- `npm run check:versions` → **green**, workspace synced to **1.4.6**.
- `plugins/channel.json` → 5 plugins (claude, codex, opencode, cursor,
  **chat-preprocessor**), all `version: 1.4.6` = app version. New chat-preprocessor
  plugin is registered in the channel and version-locked correctly.

## Blind spots / not-yet-covered (watch list)

1. **LIVE detection gates not re-run for current WIP.** stop-oracle, chat-order
   `--live`, opencode server-check require the daemon to match the uncommitted
   working tree. Offline gates and source-level tests are current.
2. **opencode /stop UNRESOLVED** (carried over): plain TUI launch, no HTTP control
   plane; raw ESC does not halt generation. Needs server-mode (`opencode serve
   --port`) + `POST /session/{id}/abort`. Not a regression — a standing gap.
3. **Strobe RED is a tracking metric, not a hard gate.** opencode question↔working
   (41) is the named open item; the claude long-chat 0ms flips are mostly transient.
   Needs the classifier-side dwell/grace work, not a loop blocker.
4. **DOM render-order ordering** is MANUAL / Windows-only (optimistic-merge path) —
   not assertable from macOS. Needs a jsdom unit or Windows Playwright DOM check.
5. **Golden ChatView** skips locally without `VITE_CODETERM_TEST`; only the ROG
   WebView2 path exercises it for real.
6. **File-size gate now green.** Remaining advisory only:
   `src-tauri/src/dispatch/mod.rs` is over the 800-line `mod.rs` target but below
   the hard 1000-line source limit.

## Round entries (before/after on Lead diffs)

_(append one block per Lead landing: diff summary, affected gates, before→after numbers)_

<!-- revision: 3 -->
