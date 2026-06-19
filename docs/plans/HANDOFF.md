# plugins-rethink Hardening — Hand-off Summary

> For Andrey. Autonomous campaign (test → review → fix → test, local macOS + ROG
> Windows) on `plugins-rethink`. **Release stays PARKED** pending your personal tests.
> Final tip: `e4a4cf82` (origin/plugins-rethink). Detailed records:
> `docs/plans/BASELINE.md` + `docs/plans/review/findings.md`.

## TL;DR
- **4 real bugs found + fixed + verified cross-platform** (2 were serious functional regressions).
- **Full review pass** (capability boundary, providers, core seams, async/locks/size) →
  every HIGH/MED finding fixed across 3 fix-waves.
- **All tests green** on the final build: local cargo **1860/0**, tsc, unit (51), plugins (31),
  all 9 check-scripts; ROG (Windows) cargo + test:plugins green.
- **No Playwright failure traced to a branch change** — all ENV/Windows/harness or pre-existing.

## Critical bugs (Phase 0/1) — fixed + verified on macOS AND Windows
- **P0.2 — agent panes didn't run.** Every non-main freshly-spawned agent pane was rejected at
  `hook_session_start` ("session does not belong to provider") → empty transcript. Cause: the CB181
  ownership check gated on `session_exists`, which is false for a brand-new session not yet on disk.
  Fix: trust the per-launch binding nonce (skip `session_exists` for our own launches; CB181 guard
  intact for nonce-less reports). Verified: fresh haiku agent binds + replies, macOS + ROG.
- **F3 — 25 parity tests failing.** claude/codex/opencode manifests lacked `hostApi:2` (T6.0a raised
  the bar; cursor had it). Fix: declared `hostApi:2`. cargo 1805/25-fail → 1830/0.
- **P0.1 — chat-UI-default broke on restore/agent-spawn.** The "default new pane to chat" pref was
  only applied on the FE spawn path; restored + CLI-agent panes never re-applied it. Fix: re-apply on
  all backend creation paths; cursor added to the bundled capability table.
- **F6 — classifiers broke on Windows (CRLF).** AS2/AS3/AS4 `classifyPaneState` matched on `\n`, so
  `\r\n` Windows pane output → unknown/low-confidence. Fix: normalize `\r\n→\n` + `.gitattributes`
  LF fixtures + CRLF regression tests. (Caught by ROG; passed macOS — the core cross-platform win.)

## Review → fix waves (Phase 2) — all merged
**Wave 1:** detection rule#3 (opencode/generic-JsProvider/cursor now refuse-ambiguous instead of
guessing newest-for-cwd) + cursor trust-prewrite gated behind `--force`; closed-history restore
robustness (reopen **race** double-poll-loop guard, **data-loss** peek-then-delete, chatBackend
`chat_view` on reopen, cascade-root activation, cycle guard); bitwarden+git hardening (no
master-password persistence, async non-blocking load = the "window-expand" fix, network-scope
validation, git repo-root cwd authority + status cache).
**Wave 2:** provider-claude.ts split 1264→683 (CI file-size); hot_reload `tick()` **lock-ordering
deadlock** fix (collect-under-short-lock) + TOCTOU + reactor lock + dead code; async/spawn_blocking
(JS-VM calls + web_routes plugin_snapshot/action/ui off the runtime, model_catalog nested-block_on
removed).
**Wave 3:** ExtensionsView refresh 60s TTL + force-refresh button; `python`→`python3` (2 CI scripts);
`build.sh` CI-mode when no-TTY; TESTING.md Windows note; **+19 native/JS parity cases** (codex/cursor/
opencode — closes rule#2 gaps; no drift found).

## Test results (final tip e4a4cf82)
- **Local (macOS):** cargo `--lib` 1860/0 · tsc clean · test:unit 51/0 · test:plugins 31/0 · all 9
  `check-*.sh` green · pre-push hook (clippy --all-targets + fmt + guards) green on every push ·
  dev smoke: agent binds + replies, 0 rejections, F4 backgrounded build works.
- **ROG (Windows):** cargo 1824/0 (pre-final) → final re-run **in progress**; test:plugins green
  (CRLF fix confirmed); P0.2 verified.
- **Playwright (ROG, Windows/WebView2 — the only place it runs):** smoke 18/10, golden 38/14 — every
  failure triaged to ENV (headless lacks LLM auth → live-agent cluster + chat-snapshot 400; no mic →
  getUserMedia; ROG's active `tailscale serve` → 2 web-access addr fails), Windows-timing (settings
  panel-open), or pre-existing/flake. **None trace to a wave-1/2/3 code change.** Definitive re-run
  on the final tip in progress.

## Residual / deferred (your call)
- **FE claude-hardcoded spawn defaults (LOW):** `spawnClaudeTab`, command-palette "Spawn Claude",
  MemoryPanel default `claude`, `host_compute.rs` default backend. Not bugs (claude is the default
  provider) but not capability-driven. Left as-is — changing spawn-button UX felt like your call.
- **daemon-shells-its-own-CLI (architectural):** `cli.rs run_plugin_cli` + web_routes plugin ops fork
  the codeterm CLI. The runtime-freeze risk is mitigated (now spawn_blocking); the proper in-process
  refactor is a larger follow-up, deferred.
- **codeterm-plugins repo:** `main` has unpushed hardening (R6 prompt-authoring + bitwarden/git) —
  **not pushed** (channel/release needs your OK). lmstudio/bitwarden/git fixes reach dev via plugin
  reinstall.

## Release readiness
Branch is materially healthier than at campaign start: the two functional regressions that would have
shipped broken (agents not running; classifiers dead on Windows) are fixed + verified on both OSes,
plus a security fix (bitwarden master-pw), a deadlock fix (hot_reload), perf (async off-runtime), and
parity discipline restored. **Release remains PARKED** for your personal tests + go. When ready, the
`release-pipeline` task un-parks (and the codeterm-plugins push/release decision is yours).

<!-- revision: 1 -->

## ROG definitive final-tip re-run — 3 pre-existing Windows/test items (NOT branch regressions)
All in areas the campaign did not touch; surfaced as bonus findings. Your call to fix:
1. **keybinding-display content != 'CTRLT' (Windows)** — settings.spec.ts:87. Element present
   (settings-keybinding-display); KeybindGroup renders the RAW config value via renderKbd
   (KeybindGroup.tsx:55) with no Mod→Ctrl resolution for display. Pre-existing (Settings/keybindings
   untouched on branch). Either a display bug (resolve `Mod` per-platform in renderKbd) or a
   mac-centric test expectation. Low.
2. **web-access serve-hint DOM-absent (2 fails)** — TEST-TESTID TYPO: web-access.spec.ts:92 queries
   `[data-testid="web-ui-serve-hint"]` but the element is `web-access-serve-hint` (NetworkTab.tsx:269).
   Trivial test fix; not a code bug. (ROG's earlier tailscale-serve hypothesis was withdrawn.)
3. **extensions addChannel closes the WebView2 page (3 fails, 3min hang)** — extensions.spec.ts:128
   addChannel → locator.click → page/context closed. No reload/navigation in app code
   (ExtensionsView addChannel just runs the add-channel action); backend stays alive. Windows/WebView2
   CDP page-lifecycle artifact under the harness. Wave-3 TTL/force-refresh didn't clear it (it's
   upstream of those assertions). Needs a Windows repro outside PW to confirm app-vs-harness.

Everything else from smoke+golden cleared on the final tip (theme-select = timing flake; live-agent/
chat-400 = no-auth harness; voice = no mic). test:plugins 31/0 + tsc clean on Windows; cargo total pending.
