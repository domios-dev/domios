# Playwright Test Audit

Generated: 2026-06-18T17:28:39.762Z

Human replay: open linked traces with `pnpm test:pw:trace <trace.zip>` or `npx playwright show-trace <trace.zip>`.

## Static Review

Files: 15. Tests: 64. Findings: 44 (22 errors, 22 warnings).

| Verdict | Test | File | Findings |
| --- | --- | --- | ---: |
| good | @smoke /api/health is ok with a build hash | tests/playwright/suites/01-smoke/smoke.spec.ts:8 | 0 |
| watch | @smoke the real app window renders the shell UI | tests/playwright/suites/01-smoke/smoke.spec.ts:15 | 1 |
| good | @smoke default workspace and at least one pane exist | tests/playwright/suites/01-smoke/smoke.spec.ts:32 | 0 |
| good | @smoke system agent providers are registered | tests/playwright/suites/01-smoke/smoke.spec.ts:41 | 0 |
| good | @smoke UI agrees with API about the active workspace | tests/playwright/suites/01-smoke/smoke.spec.ts:49 | 0 |
| good | @golden workspace create/focus/close round-trip, UI tracks it | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:36 | 0 |
| good | @golden pane create/focus/close round-trip | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:60 | 0 |
| good | @smoke @regression #132: exactly one focused pane across 50 focus switches | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:77 | 0 |
| watch | @golden workspace + pane survive a daemon kill/restart | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:95 | 1 |
| good | @smoke spawn with an unknown provider errors honestly | tests/playwright/suites/03-agents/agents.spec.ts:29 | 0 |
| good | @smoke report IPC primitive: child report lands in parent inbox (no LLM) | tests/playwright/suites/03-agents/agents.spec.ts:42 | 0 |
| good | @smoke pane status endpoint answers for a live pane and 404s for a dead id | tests/playwright/suites/03-agents/agents.spec.ts:71 | 0 |
| weak | @golden real agent spawn → works → reports back (structural asserts) | tests/playwright/suites/03-agents/agents.spec.ts:90 | 1 |
| weak | @golden multi-turn + tool round-trip observed in the live terminal DOM | tests/playwright/suites/03-agents/agents.spec.ts:181 | 2 |
| weak | @golden agent restart mid-session resumes and stays usable | tests/playwright/suites/03-agents/agents.spec.ts:216 | 3 |
| watch | @golden fully live: agent response read from the rendered terminal DOM | tests/playwright/suites/03-agents/agents.spec.ts:280 | 1 |
| watch | @golden two chat panes exchange a cross-pane message, origin rendered | tests/playwright/suites/03-agents/cross-pane-chat.spec.ts:66 | 1 |
| weak | @golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI | tests/playwright/suites/04-mesh/mesh.spec.ts:49 | 2 |
| good | @golden liar peer: alive but its services endpoint errors honestly | tests/playwright/suites/04-mesh/mesh.spec.ts:163 | 0 |
| good | @golden Scenario A: add channel and install plugin through Settings DOM | tests/playwright/suites/05-plugins/plugins.spec.ts:163 | 0 |
| good | @golden Scenario B: channel bump renders update badge, click update clears it | tests/playwright/suites/05-plugins/plugins.spec.ts:183 | 0 |
| good | @golden Scenario C: system channel update renders bundled plugins from develop | tests/playwright/suites/05-plugins/plugins.spec.ts:210 | 0 |
| good | @smoke @failure Scenario D: plugin failures are rendered in Settings DOM | tests/playwright/suites/05-plugins/plugins.spec.ts:229 | 0 |
| good | @golden save → get round-trip preserves kind/title/body | tests/playwright/suites/06-mem/mem-core.spec.ts:39 | 0 |
| good | @golden update mutates the body and history records it | tests/playwright/suites/06-mem/mem-core.spec.ts:48 | 0 |
| good | @golden search finds a freshly saved fact by distinctive terms (rank-tolerant) | tests/playwright/suites/06-mem/mem-core.spec.ts:61 | 0 |
| good | @golden delete removes the item from get and search | tests/playwright/suites/06-mem/mem-core.spec.ts:79 | 0 |
| good | @golden wake-up and stats answer with content | tests/playwright/suites/06-mem/mem-core.spec.ts:99 | 0 |
| good | @golden graph relate links two items and related returns the edge | tests/playwright/suites/06-mem/mem-core.spec.ts:107 | 0 |
| good | @smoke debug clock reports mocked state and advances | tests/playwright/suites/07-cron/cron.spec.ts:39 | 0 |
| weak | @golden oneshot cron fires via time travel and spawns a real run | tests/playwright/suites/07-cron/cron.spec.ts:46 | 3 |
| good | @golden recurring cron fires exactly per boundary, not per advance call | tests/playwright/suites/07-cron/cron.spec.ts:120 | 0 |
| good | @golden restore matrix markers are visible in their restored provider panes | tests/playwright/suites/10-restore/restore-matrix.spec.ts:63 | 0 |
| good | @smoke protected routes reject a missing bearer with 401 | tests/playwright/suites/11-api/api-core.spec.ts:43 | 0 |
| good | @smoke protected routes reject a garbage bearer with 401 | tests/playwright/suites/11-api/api-core.spec.ts:50 | 0 |
| good | @smoke public liveness routes answer without auth | tests/playwright/suites/11-api/api-core.spec.ts:57 | 0 |
| good | @golden pane ops on a nonexistent id return 404 | tests/playwright/suites/11-api/api-core.spec.ts:65 | 0 |
| weak | @golden PTY echo round-trip: send a command, read it back from output | tests/playwright/suites/11-api/api-core.spec.ts:74 | 2 |
| good | @golden workspace ops on a nonexistent id return 404, close is not idempotent-silent | tests/playwright/suites/11-api/api-core.spec.ts:103 | 0 |
| good | @golden cron CRUD lifecycle | tests/playwright/suites/11-api/api-core.spec.ts:120 | 0 |
| good | @golden cron create with a malformed schedule is rejected with 400 | tests/playwright/suites/11-api/api-core.spec.ts:143 | 0 |
| good | @golden pasteboard put → get → pop → gone | tests/playwright/suites/11-api/api-core.spec.ts:159 | 0 |
| good | @smoke activity feed returns a bounded array and accepts source filter | tests/playwright/suites/11-api/api-core.spec.ts:178 | 0 |
| good | @smoke themes list is a non-empty name array | tests/playwright/suites/11-api/api-core.spec.ts:186 | 0 |
| good | @golden upload round-trip: multipart in, exact bytes back via serve-file | tests/playwright/suites/11-api/api-core.spec.ts:195 | 0 |
| good | @golden upload rejects a missing file field and missing auth | tests/playwright/suites/11-api/api-core.spec.ts:222 | 0 |
| good | @golden fs list-dir + mkdir: browse, create, conflict, validation | tests/playwright/suites/11-api/api-core.spec.ts:243 | 0 |
| good | @smoke sessions search answers with a hits array for any query | tests/playwright/suites/11-api/api-core.spec.ts:280 | 0 |
| good | @golden pasting a screenshot into the chat composer attaches it | tests/playwright/suites/12-web/chat-image-paste.spec.ts:29 | 0 |
| weak | @golden voice note: transcribing placeholder morphs into transcript, and failure shows an honest error | tests/playwright/suites/12-web/voice-indicator.spec.ts:57 | 1 |
| weak | @golden mic stream is warm: two recordings → getUserMedia called exactly once | tests/playwright/suites/12-web/voice-warm-mic.spec.ts:54 | 1 |
| good | @smoke /web serves the SPA; token gate authenticates; self is the default host | tests/playwright/suites/12-web/web.spec.ts:41 | 0 |
| good | @golden bad token is rejected with a rendered error, not a broken app | tests/playwright/suites/12-web/web.spec.ts:56 | 0 |
| weak | @golden peer switching: dropdown lists the stub, traffic routes to it, switch-back restores | tests/playwright/suites/12-web/web.spec.ts:66 | 1 |
| weak | @golden unreachable peer fails gracefully at the gate, back-to-self recovers | tests/playwright/suites/12-web/web.spec.ts:150 | 1 |
| good | @golden mobile viewport: no horizontal scroll, sidebar overlays, picker reachable | tests/playwright/suites/12-web/web.spec.ts:168 | 0 |
| weak | @golden fully live on mobile: General Agent responds, read from the rendered DOM | tests/playwright/suites/12-web/web.spec.ts:194 | 1 |
| good | @golden share link: #token= auto-enters and the URL is redacted | tests/playwright/suites/12-web/web.spec.ts:242 | 0 |
| good | @golden share link: ?token= query form also works and is redacted | tests/playwright/suites/12-web/web.spec.ts:252 | 0 |
| good | @golden share link: a bad token falls through to the gate error path | tests/playwright/suites/12-web/web.spec.ts:260 | 0 |
| good | @golden web chrome: titlebar absent, sidebar hosts peer picker + keep-awake | tests/playwright/suites/12-web/web.spec.ts:271 | 0 |
| good | @golden web mobile: hamburger reopens the dismissed sidebar overlay | tests/playwright/suites/12-web/web.spec.ts:295 | 0 |
| weak | @golden close sync: web close updates the desktop window, desktop close updates web | tests/playwright/suites/12-web/web.spec.ts:313 | 3 |
| weak | @golden web chat: a live assistant reply streams in AND history survives a reload | tests/playwright/suites/12-web/web.spec.ts:363 | 1 |

### Findings

| Severity | Rule | Location | Test | Message |
| --- | --- | --- | --- | --- |
| warn | brittle-selector:dot-first | tests/playwright/suites/01-smoke/smoke.spec.ts:25 | @smoke the real app window renders the shell UI | selector starts from a CSS class without a unique key: .xterm:visible |
| warn | inline-fixture:api-client | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:13 | - | spec creates ApiClient directly instead of using the api fixture |
| warn | inline-fixture:connect-ui | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:17 | - | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:connect-ui | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:110 | @golden workspace + pane survive a daemon kill/restart | spec calls connectUi() directly instead of using the ui fixture |
| error | missing-expect | tests/playwright/suites/03-agents/agents.spec.ts:181 | @golden multi-turn + tool round-trip observed in the live terminal DOM | test has no expect(): @golden multi-turn + tool round-trip observed in the live terminal DOM |
| warn | inline-fixture:api-client | tests/playwright/suites/03-agents/agents.spec.ts:21 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/03-agents/agents.spec.ts:135 | @golden real agent spawn → works → reports back (structural asserts) | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:connect-ui | tests/playwright/suites/03-agents/agents.spec.ts:187 | @golden multi-turn + tool round-trip observed in the live terminal DOM | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:connect-ui | tests/playwright/suites/03-agents/agents.spec.ts:219 | @golden agent restart mid-session resumes and stays usable | spec calls connectUi() directly instead of using the ui fixture |
| error | sleep:setTimeout | tests/playwright/suites/03-agents/agents.spec.ts:250 | @golden agent restart mid-session resumes and stays usable | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| error | sleep:setTimeout | tests/playwright/suites/03-agents/agents.spec.ts:256 | @golden agent restart mid-session resumes and stays usable | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:connect-ui | tests/playwright/suites/03-agents/agents.spec.ts:286 | @golden fully live: agent response read from the rendered terminal DOM | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:api-client | tests/playwright/suites/03-agents/cross-pane-chat.spec.ts:21 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/03-agents/cross-pane-chat.spec.ts:44 | - | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:connect-ui | tests/playwright/suites/03-agents/cross-pane-chat.spec.ts:70 | @golden two chat panes exchange a cross-pane message, origin rendered | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:api-client | tests/playwright/suites/04-mesh/mesh.spec.ts:17 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/04-mesh/mesh.spec.ts:45 | - | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| error | sleep:setTimeout | tests/playwright/suites/04-mesh/mesh.spec.ts:87 | @golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:connect-ui | tests/playwright/suites/04-mesh/mesh.spec.ts:96 | @golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:connect-ui | tests/playwright/suites/05-plugins/plugins.spec.ts:121 | - | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:api-client | tests/playwright/suites/07-cron/cron.spec.ts:11 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/07-cron/cron.spec.ts:75 | @golden oneshot cron fires via time travel and spawns a real run | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| error | sleep:setTimeout | tests/playwright/suites/07-cron/cron.spec.ts:85 | @golden oneshot cron fires via time travel and spawns a real run | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| error | sleep:setTimeout | tests/playwright/suites/07-cron/cron.spec.ts:108 | @golden oneshot cron fires via time travel and spawns a real run | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:api-client | tests/playwright/suites/10-restore/restore-matrix.spec.ts:31 | - | spec creates ApiClient directly instead of using the api fixture |
| warn | inline-fixture:connect-ui | tests/playwright/suites/10-restore/restore-matrix.spec.ts:35 | - | spec calls connectUi() directly instead of using the ui fixture |
| warn | inline-fixture:api-client | tests/playwright/suites/11-api/api-core.spec.ts:15 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/11-api/api-core.spec.ts:82 | @golden PTY echo round-trip: send a command, read it back from output | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| error | sleep:setTimeout | tests/playwright/suites/11-api/api-core.spec.ts:90 | @golden PTY echo round-trip: send a command, read it back from output | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:api-client | tests/playwright/suites/12-web/chat-image-paste.spec.ts:15 | - | spec creates ApiClient directly instead of using the api fixture |
| warn | inline-fixture:api-client | tests/playwright/suites/12-web/voice-indicator.spec.ts:17 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:waitForTimeout | tests/playwright/suites/12-web/voice-indicator.spec.ts:53 | - | waitForTimeout is a hard sleep; use web-first assertions or pollUntil |
| error | sleep:setTimeout | tests/playwright/suites/12-web/voice-indicator.spec.ts:98 | @golden voice note: transcribing placeholder morphs into transcript, and failure shows an honest error | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:api-client | tests/playwright/suites/12-web/voice-warm-mic.spec.ts:12 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:waitForTimeout | tests/playwright/suites/12-web/voice-warm-mic.spec.ts:50 | - | waitForTimeout is a hard sleep; use web-first assertions or pollUntil |
| error | sleep:waitForTimeout | tests/playwright/suites/12-web/voice-warm-mic.spec.ts:100 | @golden mic stream is warm: two recordings → getUserMedia called exactly once | waitForTimeout is a hard sleep; use web-first assertions or pollUntil |
| error | missing-expect | tests/playwright/suites/12-web/web.spec.ts:150 | @golden unreachable peer fails gracefully at the gate, back-to-self recovers | test has no expect(): @golden unreachable peer fails gracefully at the gate, back-to-self recovers |
| error | missing-expect | tests/playwright/suites/12-web/web.spec.ts:194 | @golden fully live on mobile: General Agent responds, read from the rendered DOM | test has no expect(): @golden fully live on mobile: General Agent responds, read from the rendered DOM |
| warn | inline-fixture:api-client | tests/playwright/suites/12-web/web.spec.ts:13 | - | spec creates ApiClient directly instead of using the api fixture |
| error | sleep:setTimeout | tests/playwright/suites/12-web/web.spec.ts:100 | @golden peer switching: dropdown lists the stub, traffic routes to it, switch-back restores | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |
| warn | inline-fixture:connect-ui | tests/playwright/suites/12-web/web.spec.ts:325 | @golden close sync: web close updates the desktop window, desktop close updates web | spec calls connectUi() directly instead of using the ui fixture |
| error | force-action | tests/playwright/suites/12-web/web.spec.ts:340 | @golden close sync: web close updates the desktop window, desktop close updates web | force: true masks timing/actionability problems |
| error | force-action | tests/playwright/suites/12-web/web.spec.ts:348 | @golden close sync: web close updates the desktop window, desktop close updates web | force: true masks timing/actionability problems |
| error | sleep:setTimeout | tests/playwright/suites/12-web/web.spec.ts:393 | @golden web chat: a live assistant reply streams in AND history survives a reload | bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion |

## Trace Review

Trace ZIPs: 0. Parsed records: 0. Errors: 0.

No trace artifacts found under `tests/playwright/artifacts`.

