# Live 3-Provider QA + Restart/Resume — Execution Plan

> Autonomous live QA. I am the judge: open real panes in dev, spawn each
> provider, send messages, READ their answers, judge output/parse quality,
> fix prompts/parsers/backend when behaviour diverges, retest. No subagent
> scripts standing in for judgement.

**Dev only:** port 7686, token `3bc06f93…` (from `~/.codeterm-dev/config.toml`).
Never touch prod 7685. Reload webview via CDP (`http://127.0.0.1:9223`) after any Rust restart.

---

## Phase 0 — Harness up (done-criteria: I can drive dev headless)
- [ ] `source evals/_lib/lib.sh` → `dc` wrapper locked to dev (exit 7 if dev down)
- [ ] Confirm dev build hash == HEAD-with-my-changes (08288f9a)
- [ ] Helper to: create workspace, open pane, spawn provider, send text, read chat/screen
- [ ] Enumerate current panes; clean up test debris from the killed regression
      (the `pw-web-agent` report leaked into Main as a raw shell command)

## Phase 1 — BUG: report marker leaked into a shell pane (from screenshot)
**Symptom:** `-=-codeterm:report from=pw-web-agent pane=2405efcd-=-STATUS:` was
pasted into Main's PowerShell and executed as a command → CommandNotFound.
**Hypothesis:** report delivery to the parent pane does not gate on "recipient is a
live agent". Shell recipients must get NO marker (or report must refuse shell targets).
- [ ] Find report delivery path (`codeterm agent report` → REST → parent pane inject)
- [ ] Confirm whether it reuses the send-text shell-vs-agent guard
- [ ] Fix: shell/non-agent recipient → no marker prefix (mirror send_text behaviour),
      or drop the report into chat-only channel, never the PTY of a bare shell
- [ ] Regression test for it
- [ ] Rebuild, reload, reproduce-then-verify-green

## Phase 2 — Live provider review (claude, codex, opencode)
For EACH provider, in its own workspace/cwd:
- [ ] Spawn provider in a fresh pane; confirm it actually STARTED (not bare shell)
      via `detect_started_session` populating `agent_session_id`
- [ ] Send a concrete question (e.g. "what is 17*23? then run `pwd`")
- [ ] READ the chat tail — judge:
      - assistant text rendered correctly (markdown, no marker leakage)
      - tool calls render richly (Bash/shell → `Bash(cmd)` pill, not empty)
      - tool results attached under the call
- [ ] Cross-pane: from provider A send a message to provider B's pane; verify the
      `-=-codeterm:…-=-` marker is parsed into an origin header with the PROVIDER
      icon (not a letter glyph) and "from <Name>" — NOT raw text in the shell
- [ ] Judge codex specifically (was the weak one): shell_command JSONL → `Bash(...)`
- [ ] If behaviour diverges from prompts, fix the prompt/parser and retest

## Phase 3 — Restart / session-resume (railroad-solid for all 3)
For EACH provider:
- [ ] Spawn provider, send a message, let a session file be written
- [ ] Note pane→session binding (`agent_session_id`)
- [ ] `./build.sh kill` then `./build.sh dev --wait` (full restart)
- [ ] Reload webview (CDP), confirm:
      - Main comes back as the GA agent titled "Main" (eager-mounted, not bare shell)
      - each provider pane re-binds to its SAME session id (no random/foreign latch)
      - chat history reappears (tailer re-reads the right JSONL)
      - sending a new message continues the SAME conversation
- [ ] Multiple projects: ≥2 workspaces with different cwds, each with a provider,
      survive restart with correct per-pane session binding
- [ ] If a pane latches a foreign/empty session → that's the #43 failure; fix & retest

## Phase 4 — Multi-send + composer sanity (live)
- [ ] Send 3 messages rapidly without waiting; all delivered, no lost/dup
- [ ] Desktop: no top burger bar; sidebar grabber pip resizes + collapses
- [ ] Main tab title stays "Main"; `--pane main` resolves; rename blocked

## Phase 5 — Commit gate
- [ ] Run unit tests (`./build.sh test`) + targeted Playwright green
- [ ] Commit fixes in logical chunks; push; (merge to develop already done earlier)
- [ ] Final written verdict per provider: PASS/FAIL with the evidence I read

---

### Running log (cont. 06-10)
- web chat composer "no reply" = the write raced claude's TUI boot; bracket-paste
  before the input was ready got swallowed. Fixed: web raw=false writes route
  through send_text_to_pty_smart (verified injection: boot-wait +
  await_agent_tui_on_screen fingerprint gate + escalating-Enter retries). First
  attempt used require_prompt_first which false-timed-out on claude's footer (last
  screen line is the footer, not ❯) and broke desktop agent sends — replaced with
  the TUI-fingerprint gate. web chat green; live agent send green.
- 03-agents live-terminal-DOM failures (178/275) were NOT a code regression:
  test pollution. My runs left `codeterm-chat-ui-default-enabled=true` in the dev
  desktop app's localStorage, so every new agent pane opened in CHAT mode (no
  xterm a11y tree the tests read). Backend delivery proven fine (marker in PTY in
  5s). Reset the default → re-running. Test-hygiene gap: these connectUi tests
  should force terminal mode in setup.

### Running log
- **Phase 1 root-caused + fixed:** `hierarchy.rs submit_report` injected the report
  marker into the target PTY unconditionally → pasted into bare PowerShell when the
  agent had died. Added `target_has_live_agent` gate (screen TUI fingerprints).
  Live verify still pending.
- **Phase 3 BUG #1 (the big one):** GA Main never resumed on Windows. Root cause:
  `std::fs::canonicalize` returns `\\?\`-verbatim paths on Windows; both
  `agent_claude/provider/sessions.rs` AND the duplicated logic in
  `js_runtime/provider.rs` encoded it into a claude project dir with 4 extra
  leading dashes (`----C--Users-…`) → `session_exists`=false → always fresh launch.
  Fixed via shared `paths::canonical_cwd` / `paths::strip_verbatim_prefix`,
  used by both. 6 unit tests green.
- **Phase 3 verify:** restart #1 → `claude --resume 449db8e0` (first ever resume on
  this machine); control restart #2 → SAME sid resumed (idempotent). opencode kept
  `ses_1525…` across all restarts. Main screen shows replayed history, not onboarding.
- DSR/headless fix from earlier session confirmed live (PTY echo test green).
- **Phase 2 BUG #2 (session theft, live-confirmed):** fresh codex pane latched the
  prod instance's session (same repo cwd). Two holes: js-plugin wrapper sent
  max_age=None as 0 → "no recency cutoff" → any stale rollout won; and
  launched-detection filtered by mtime, which a constantly-written foreign session
  always passes. Fixed: wrapper None→300s default + created-time gate in
  detect_launched (wrapper + native codex). Verified live: probe pane claimed only
  the rollout born at its own launch; zombie-pane exclusion then rightful re-claim
  in 5s after zombie close.
- **Phase 2 BUG #3 (orchestrator spawns hang):** agents spawned via API/orchestrator
  showed a blank pane — backend PTY's CSI 6n unanswered (background tab never mounts
  an xterm). Fixed via resize-owner-aware DSR answer; codex probe then ran and
  answered correctly (17*23=391 + cwd name via shell — judged the live output).
- **Phase 1 verified live, both branches:** report → dead shell target = stored
  only, no PTY spawn/paste (screenshot bug dead); report → live Main = injected,
  claude recognized the report marker and applied routing rules (no reply — per
  prompt). Required fixing stale claude TUI fingerprints (v2.1 footer + banner
  glyphs, native+js parity).
- **codex tool rendering re-judged:** backend ChatMsg carries raw tool_name +
  toolInput; MessageBubble normalizes shell_command→shell pill with the command
  from toolInput.command. Render chain intact; earlier "empty content" was the
  wire format, not a bug.
- **Noted, not fixed (scope):** pane hierarchy metadata (parent/report_target/
  spawned_by) does not survive daemon restart — orchestration links are
  session-scoped today.
- 3-provider restart matrix passed: claude --resume same sid ×3 restarts;
  opencode same ses_… across all; codex re-bound same rollout uuid.
- **Phase 4 web triage:** mobile-viewport fail = real CSS regression — phone
  button-shrink block in globals.css nuked deliberate `min-h-[44px]`/`h-[44px]`
  touch targets to 25px; arbitrary-value classes now exempt. PASS.
- mobile-GA test was stale product-wise (mobile defaults supported panes to
  chat → no xterm a11y tree); test now accepts the marker from chat bubbles
  too. PASS.
- **web chat composer chain (3 stacked bugs):** (1) commit be7d5557 made pane
  UI mode client-local and orphaned the REST /chat-view endpoint — restored as
  a remote-control: endpoint broadcasts `pane-chat-view`, TerminalPane listens;
  (2) backend `pane.chat_view` now also seeds the initial mode (client-local
  choice still wins); (3) `chatSupported` used a non-reactive descriptor-cache
  read — panes rendered before descriptors loaded froze terminal-only; now
  reactive via useAgentDescriptor.
- **Operational gotcha (cost ~3 test cycles):** the daemon serves /web from a
  bundle EMBEDDED at compile time — the dist/ vite watcher is necessary but not
  sufficient; web SPA changes need `./build.sh dev` (daemon rebuild) to reach
  Playwright web tests.
