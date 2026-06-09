# Full Regression + Live Agent Review — Autonomous QA Plan

> **For agentic workers:** execute inline (I am the reviewer/judge — no review-subagents, no scripts-that-run-agents). Test the live dev console directly, query agents, read their answers, judge output/parsing, fix prompts/code on divergence, retest.

**Goal:** Verify the whole CodeTerm self-build is solid — automated regression green, the three providers (claude/codex/opencode) render & parse well in the chat UI, and console restart resumes sessions rock-solid across providers and projects. Fix what's broken in the same loop.

**Architecture:** Land the accumulated working tree on `develop`, run dev FROM develop, then drive everything through the dev daemon's REST/CLI (`http://127.0.0.1:7686`, dev token) + CDP webview reads. Judge by reading actual agent responses & rendered DOM, not by trusting "OK (sent)".

**Tech Stack:** Rust (cargo via build.sh), Playwright (`node node_modules/@playwright/test/cli.js test -c tests/playwright --reporter=list`), tsx for TS unit tests, CDP (msedge `connectOverCDP :9223`) for desktop webview, REST/CLI for agent drive.

---

## Phase 0 — Land on develop (pre-test)

- [ ] Build health: `./build.sh kill && ./build.sh check` (Finished) + `pnpm exec tsc --noEmit` (clean). DONE this session.
- [ ] Rust tests: `./build.sh test` — capture pass count; the only acceptable red is the pre-existing `git_routes::revert_restores_tracked_file` (Windows autocrlf, not ours).
- [ ] TS parser tests: `node node_modules/.bin/tsx.CMD src/lib/chatPrefixes.test.ts`; provider parser tests: `node node_modules/.bin/tsx.CMD plugins/provider-codex/tests/parser.test.ts` and `plugins/provider-claude/tests/parser.test.ts`.
- [ ] Commit the accumulated tree on `feat/system-plugins-intrinsic` (logical messages), push.
- [ ] Merge `feat/system-plugins-intrinsic` → `develop`, push. From here on, dev runs develop.
- [ ] Restart dev FROM develop with CDP: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 VITE_CODETERM_TEST=1 CODETERM_TEST_CLOCK=1 ./build.sh dev --wait`; reload webview via CDP.

## Phase 1 — Automated regression

- [ ] Playwright full suite: `node node_modules/@playwright/test/cli.js test -c tests/playwright --reporter=list`. Triage each failure: real bug → fix; flaky/env → note + rerun once. Known flake: `04-mesh stub peer hysteresis`.
- [ ] Re-run any fixed suite to confirm green.

## Phase 2 — Live agent behavior + parsing review (I am the judge)

For EACH provider — claude, codex, opencode — in the General workspace (dev):
- [ ] Confirm a pane of that provider exists/started (or spawn: `codeterm agent spawn <provider> --title <T> --workspace <General>`); verify it ACTUALLY started (TUI markers present, not just a pane id).
- [ ] Send a probe via REST/CLI that forces a tool call + text: e.g. "list the files in your cwd with a shell command, then say DONE-<marker>". Read the response:
  - `/api/panes/{id}/chat-raw/start` (raw JSONL window) AND `/api/panes/{id}/text?formatted=false` (screen) to see what arrived.
  - Read the rendered chat DOM via CDP (`.xterm-accessibility` / chat-message testids) to judge: is the tool call rendered richly (command visible, not bare `shell_command()`)? Is the `[codeterm from …]` prefix parsed into a header (not raw)? Model avatar = provider icon?
- [ ] Cross-pane: from Main send `codeterm send "<marker> ping, reply with <marker2>" --pane <provider-pane>`; read the provider pane — did the prefix arrive intact (`[codeterm from Main …]`, brackets present), parse to a header, and did the agent reply correctly?
- [ ] Judge against prompts: if an agent mis-routes a reply, double-answers, or ignores the prefix → the prompt (general_agent.md / codeterm_cli.md / worker/orchestrator suffix) is the fix, not the code. Edit the prompt, rebuild (Rust include_str), restart, reprobe.
- [ ] Record per-provider verdict: started ✓/✗, tool render ✓/✗, prefix parse ✓/✗, avatar ✓/✗, reply correctness ✓/✗.

## Phase 3 — Restart / session-resume (rock-solid × 3 providers × projects)

- [ ] For each provider pane: send a UNIQUE marker message ("REMEMBER-<provider>-<ts>"), confirm the agent saw it.
- [ ] `POST /api/session/save`, then `./build.sh kill` (force) + restart dev FROM develop, reload webview.
- [ ] After restart: the SAME pane must (a) restore with the same provider/title/model, (b) resume the SAME session (not latch a foreign one — the #43 concern), (c) the chat history must still contain the marker. Read chat-raw + DOM to confirm.
- [ ] Multi-project: create a second workspace in a different cwd (e.g. a real repo), spawn one provider, send a marker, restart, confirm resume there too. Repeat for at least claude + codex + opencode across ≥2 cwds.
- [ ] If resume latches a random/empty session or loses history → fix per #43 (pane-owned session id, wait-for-real-start, empty-until-ready). Retest the exact scenario.

## Phase 4 — Fix → retest loop

- [ ] For every fix: rebuild (Rust → kill+dev+reload; TS → HMR but reload webview to be safe), rerun ONLY the failing probe/suite, then a quick smoke of adjacent areas.
- [ ] When all phases green, final commit + push (+ merge to develop if more landed). Write a concise verdict report.

## Guardrails
- Never touch prod (7685). Dev = 7686. `./build.sh kill` is dev-only.
- After EVERY Rust restart, reload the webview via CDP (Tauri doesn't auto-reload on Vite restart).
- No giant comments, English code, 1000-line file limit. No auto-commit beyond what this plan authorizes (Andrey granted commit/push/merge-to-dev for this run).
- Judge by reading real output, never by "OK (sent)".
