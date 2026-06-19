# Playwright Test Audit

Generated: 2026-06-19T01:30:07.997Z

Human replay: open linked traces with `pnpm test:pw:trace <trace.zip>` or `npx playwright show-trace <trace.zip>`.

## Static Review

Files: 20. Tests: 78. Findings: 0 (0 errors, 0 warnings).

| Verdict | Test | File | Findings |
| --- | --- | --- | ---: |
| good | @smoke /api/health is ok with a build hash | tests/playwright/suites/01-smoke/smoke.spec.ts:8 | 0 |
| good | @smoke the real app window renders the shell UI | tests/playwright/suites/01-smoke/smoke.spec.ts:15 | 0 |
| good | @smoke default workspace and at least one pane exist | tests/playwright/suites/01-smoke/smoke.spec.ts:36 | 0 |
| good | @smoke system agent providers are registered | tests/playwright/suites/01-smoke/smoke.spec.ts:45 | 0 |
| good | @smoke UI agrees with API about the active workspace | tests/playwright/suites/01-smoke/smoke.spec.ts:53 | 0 |
| good | @golden workspace create/focus/close round-trip, UI tracks it | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:20 | 0 |
| good | @golden pane create/focus/close round-trip | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:44 | 0 |
| good | @smoke @regression #132: exactly one focused pane across 50 focus switches | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:61 | 0 |
| good | @golden workspace + pane survive a daemon kill/restart | tests/playwright/suites/02-multiplexer/multiplexer.spec.ts:79 | 0 |
| good | @smoke spawn with an unknown provider errors honestly | tests/playwright/suites/03-agents/agents.spec.ts:21 | 0 |
| good | @smoke report IPC primitive: child report lands in parent inbox (no LLM) | tests/playwright/suites/03-agents/agents.spec.ts:34 | 0 |
| good | @smoke pane status endpoint answers for a live pane and 404s for a dead id | tests/playwright/suites/03-agents/agents.spec.ts:58 | 0 |
| good | @golden real agent spawn → works → reports back (structural asserts) | tests/playwright/suites/03-agents/agents.spec.ts:74 | 0 |
| good | @golden multi-turn + tool round-trip observed in the live terminal DOM | tests/playwright/suites/03-agents/agents.spec.ts:146 | 0 |
| good | @golden agent restart mid-session resumes and stays usable | tests/playwright/suites/03-agents/agents.spec.ts:185 | 0 |
| good | @golden fully live: agent response read from the rendered terminal DOM | tests/playwright/suites/03-agents/agents.spec.ts:248 | 0 |
| good | @golden two chat panes exchange a cross-pane message, origin rendered | tests/playwright/suites/03-agents/cross-pane-chat.spec.ts:59 | 0 |
| good | @golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI | tests/playwright/suites/04-mesh/mesh.spec.ts:31 | 0 |
| good | @golden liar peer: alive but its services endpoint errors honestly | tests/playwright/suites/04-mesh/mesh.spec.ts:149 | 0 |
| good | @golden Scenario A: add channel and install plugin through Settings DOM | tests/playwright/suites/05-plugins/plugins.spec.ts:168 | 0 |
| good | @golden Scenario B: channel bump renders update badge, click update clears it | tests/playwright/suites/05-plugins/plugins.spec.ts:184 | 0 |
| good | @golden Scenario C: system channel update renders bundled plugins from develop | tests/playwright/suites/05-plugins/plugins.spec.ts:207 | 0 |
| good | @smoke @failure Scenario D: plugin failures are rendered in Settings DOM | tests/playwright/suites/05-plugins/plugins.spec.ts:222 | 0 |
| good | @golden save → get round-trip preserves kind/title/body | tests/playwright/suites/06-mem/mem-core.spec.ts:39 | 0 |
| good | @golden update mutates the body and history records it | tests/playwright/suites/06-mem/mem-core.spec.ts:48 | 0 |
| good | @golden search finds a freshly saved fact by distinctive terms (rank-tolerant) | tests/playwright/suites/06-mem/mem-core.spec.ts:61 | 0 |
| good | @golden delete removes the item from get and search | tests/playwright/suites/06-mem/mem-core.spec.ts:79 | 0 |
| good | @golden wake-up and stats answer with content | tests/playwright/suites/06-mem/mem-core.spec.ts:99 | 0 |
| good | @golden graph relate links two items and related returns the edge | tests/playwright/suites/06-mem/mem-core.spec.ts:107 | 0 |
| good | @smoke debug clock reports mocked state and advances | tests/playwright/suites/07-cron/cron.spec.ts:38 | 0 |
| good | @golden oneshot cron fires via time travel and spawns a real run | tests/playwright/suites/07-cron/cron.spec.ts:45 | 0 |
| good | @golden recurring cron fires exactly per boundary, not per advance call | tests/playwright/suites/07-cron/cron.spec.ts:116 | 0 |
| good | @golden restore matrix markers are visible in their restored provider panes | tests/playwright/suites/10-restore/restore-matrix.spec.ts:50 | 0 |
| good | @smoke protected routes reject a missing bearer with 401 | tests/playwright/suites/11-api/api-core.spec.ts:35 | 0 |
| good | @smoke protected routes reject a garbage bearer with 401 | tests/playwright/suites/11-api/api-core.spec.ts:42 | 0 |
| good | @smoke public liveness routes answer without auth | tests/playwright/suites/11-api/api-core.spec.ts:49 | 0 |
| good | @golden pane ops on a nonexistent id return 404 | tests/playwright/suites/11-api/api-core.spec.ts:57 | 0 |
| good | @golden PTY echo round-trip: send a command, read it back from output | tests/playwright/suites/11-api/api-core.spec.ts:66 | 0 |
| good | @golden workspace ops on a nonexistent id return 404, close is not idempotent-silent | tests/playwright/suites/11-api/api-core.spec.ts:91 | 0 |
| good | @golden cron CRUD lifecycle | tests/playwright/suites/11-api/api-core.spec.ts:108 | 0 |
| good | @golden cron create with a malformed schedule is rejected with 400 | tests/playwright/suites/11-api/api-core.spec.ts:131 | 0 |
| good | @golden pasteboard put → get → pop → gone | tests/playwright/suites/11-api/api-core.spec.ts:147 | 0 |
| good | @smoke activity feed returns a bounded array and accepts source filter | tests/playwright/suites/11-api/api-core.spec.ts:166 | 0 |
| good | @smoke themes list is a non-empty name array | tests/playwright/suites/11-api/api-core.spec.ts:174 | 0 |
| good | @golden upload round-trip: multipart in, exact bytes back via serve-file | tests/playwright/suites/11-api/api-core.spec.ts:183 | 0 |
| good | @golden upload rejects a missing file field and missing auth | tests/playwright/suites/11-api/api-core.spec.ts:210 | 0 |
| good | @golden fs list-dir + mkdir: browse, create, conflict, validation | tests/playwright/suites/11-api/api-core.spec.ts:231 | 0 |
| good | @smoke sessions search answers with a hits array for any query | tests/playwright/suites/11-api/api-core.spec.ts:268 | 0 |
| good | @golden pasting a screenshot into the chat composer attaches it | tests/playwright/suites/12-web/chat-image-paste.spec.ts:18 | 0 |
| good | @golden voice note: transcribing placeholder morphs into transcript, and failure shows an honest error | tests/playwright/suites/12-web/voice-indicator.spec.ts:48 | 0 |
| good | @golden mic stream is warm: two recordings → getUserMedia called exactly once | tests/playwright/suites/12-web/voice-warm-mic.spec.ts:45 | 0 |
| good | @smoke /web serves the SPA; token gate authenticates; self is the default host | tests/playwright/suites/12-web/web.spec.ts:30 | 0 |
| good | @golden bad token is rejected with a rendered error, not a broken app | tests/playwright/suites/12-web/web.spec.ts:45 | 0 |
| good | @golden peer switching: dropdown lists the stub, traffic routes to it, switch-back restores | tests/playwright/suites/12-web/web.spec.ts:55 | 0 |
| good | @golden unreachable peer fails gracefully at the gate, back-to-self recovers | tests/playwright/suites/12-web/web.spec.ts:136 | 0 |
| good | @golden mobile viewport: no horizontal scroll, sidebar overlays, picker reachable | tests/playwright/suites/12-web/web.spec.ts:156 | 0 |
| good | @golden fully live on mobile: General Agent responds, read from the rendered DOM | tests/playwright/suites/12-web/web.spec.ts:182 | 0 |
| good | @golden share link: #token= auto-enters and the URL is redacted | tests/playwright/suites/12-web/web.spec.ts:220 | 0 |
| good | @golden share link: ?token= query form also works and is redacted | tests/playwright/suites/12-web/web.spec.ts:230 | 0 |
| good | @golden share link: a bad token falls through to the gate error path | tests/playwright/suites/12-web/web.spec.ts:238 | 0 |
| good | @golden web chrome: titlebar absent, sidebar hosts peer picker + keep-awake | tests/playwright/suites/12-web/web.spec.ts:249 | 0 |
| good | @golden web mobile: hamburger reopens the dismissed sidebar overlay | tests/playwright/suites/12-web/web.spec.ts:273 | 0 |
| good | @golden close sync: web close updates the desktop window, desktop close updates web | tests/playwright/suites/12-web/web.spec.ts:291 | 0 |
| good | @golden web chat: a live assistant reply streams in AND history survives a reload | tests/playwright/suites/12-web/web.spec.ts:344 | 0 |
| good | @smoke theme selection applies to live config and CSS variables | tests/playwright/suites/13-settings/settings.spec.ts:44 | 0 |
| good | @smoke font family edit persists through the Settings UI | tests/playwright/suites/13-settings/settings.spec.ts:72 | 0 |
| good | @smoke keybinding display renders configured shortcuts | tests/playwright/suites/13-settings/settings.spec.ts:87 | 0 |
| good | @golden MemoryPanel searches saved memory and opens item preview | tests/playwright/suites/14-memory-ui/memory-panel.spec.ts:43 | 0 |
| good | @golden MemoryPanel renders graph relations in item detail | tests/playwright/suites/14-memory-ui/memory-panel.spec.ts:68 | 0 |
| good | @smoke list and detail panel render channel extension metadata | tests/playwright/suites/15-extensions/extensions.spec.ts:170 | 0 |
| good | @golden installed extension exposes Configure with AI and plugin activity toast | tests/playwright/suites/15-extensions/extensions.spec.ts:184 | 0 |
| good | @smoke uninstall confirmation includes remove-data checkbox | tests/playwright/suites/15-extensions/extensions.spec.ts:224 | 0 |
| good | @golden ChatView renders message blocks, code blocks, and image attachments from mock replies | tests/playwright/suites/16-chat-view/chat-rendering.spec.ts:54 | 0 |
| good | @smoke lmstudio open shell spawns through the web mirror | tests/playwright/suites/17-open-shell/lmstudio.spec.ts:122 | 0 |
| good | @smoke lmstudio system prompt marker renders as a collapsible card | tests/playwright/suites/17-open-shell/lmstudio.spec.ts:141 | 0 |
| good | @smoke lmstudio mocked streaming assistant reply updates in place | tests/playwright/suites/17-open-shell/lmstudio.spec.ts:174 | 0 |
| good | @smoke lmstudio mocked tool result renders | tests/playwright/suites/17-open-shell/lmstudio.spec.ts:223 | 0 |
| good | @smoke lmstudio config edit preserves open shell transcript | tests/playwright/suites/17-open-shell/lmstudio.spec.ts:248 | 0 |

### Findings

No static findings.

## Trace Review

Trace ZIPs: 10. Parsed records: 10. Errors: 0.

| Verdict | Score | Test | Actions | Assertions | Waits | Sleeps | Screens | Trace |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| good | 100 | 17-open-shell/lmstudio.spec.ts:141 › @smoke lmstudio system prompt marker renders as a collapsible card | 7 | 11 | 12 | 0 | 33 | [trace.zip](tests/playwright/artifacts/html-report/data/26c9a652b173cee106cf9498a5060f8dbfc76519.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:223 › @smoke lmstudio mocked tool result renders | 3 | 7 | 8 | 0 | 13 | [trace.zip](tests/playwright/artifacts/html-report/data/72f097773585929f851f489ce843834bfc42b494.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:174 › @smoke lmstudio mocked streaming assistant reply updates in place | 3 | 9 | 10 | 0 | 13 | [trace.zip](tests/playwright/artifacts/html-report/data/89e409d54498cfb3d2a41d3761e2d6a9cc6a75ed.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:248 › @smoke lmstudio config edit preserves open shell transcript | 3 | 10 | 11 | 0 | 28 | [trace.zip](tests/playwright/artifacts/html-report/data/9ace9c4ed044b35f202c16330753132b5258d7b8.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:122 › @smoke lmstudio open shell spawns through the web mirror | 3 | 9 | 10 | 0 | 28 | [trace.zip](tests/playwright/artifacts/html-report/data/e71d9db0d09eec66be469445f4778cb8735c6b20.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:141 › @smoke lmstudio system prompt marker renders as a collapsible card | 7 | 11 | 12 | 0 | 33 | [trace.zip](tests/playwright/artifacts/test-results/17-open-shell-lmstudio--sm-1bec6-nders-as-a-collapsible-card-smoke/trace.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:223 › @smoke lmstudio mocked tool result renders | 3 | 7 | 8 | 0 | 13 | [trace.zip](tests/playwright/artifacts/test-results/17-open-shell-lmstudio--sm-389da--mocked-tool-result-renders-smoke/trace.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:122 › @smoke lmstudio open shell spawns through the web mirror | 3 | 9 | 10 | 0 | 28 | [trace.zip](tests/playwright/artifacts/test-results/17-open-shell-lmstudio--sm-3efab-awns-through-the-web-mirror-smoke/trace.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:248 › @smoke lmstudio config edit preserves open shell transcript | 3 | 10 | 11 | 0 | 28 | [trace.zip](tests/playwright/artifacts/test-results/17-open-shell-lmstudio--sm-50809-erves-open-shell-transcript-smoke/trace.zip) |
| good | 100 | 17-open-shell/lmstudio.spec.ts:174 › @smoke lmstudio mocked streaming assistant reply updates in place | 3 | 9 | 10 | 0 | 13 | [trace.zip](tests/playwright/artifacts/test-results/17-open-shell-lmstudio--sm-a565b-tant-reply-updates-in-place-smoke/trace.zip) |

