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
