# B2 isolated browser driver

RC source root: /Users/dev/Developer/codeterm-worktrees/rc-mac. Scripts are pad-only. The owner checkout is untouched. This fresh assignment explicitly authorizes isolated headless Chromium against aiderqa/7697 and BLACKSTAR Windows DEV/7686. Windows lifecycle belongs to Fermi via Euler. No live actions until GO live with exact Windows DEV ID. The source-only Worker has executed no gates or browser tests.

## Exact live command after GO

    B2_GO_LIVE=yes bash evals/mesh-webui/live.sh EXACT_WINDOWS_DEV_PEER_ID evals/mesh-webui/live-manifest.json B2_UNIQUE_RUN

Populate live-manifest.json from manifest.example.json. Copy each home machine's fixture inventory `workspace_id` into its manifest entry; individual tab/shell/terminal entries may supply their own `workspace_id` when different. The shared selector activates this workspace through the UI before requiring the fixture row to be visible. It must name both exact paired IDs, four explicitly test-owned provider tabs on each machine, exact claimed session IDs, exact native paths, controller titles, actual cheap model catalogue IDs, and protected gateway token files. The Windows gateway token must be supplied by its local owner/authorized setup; it is never fetched through a remote secrets route. Windows native export needs a test-owned Windows DEV shell ID and a working python/python3/py executable. No fixture spawns are hidden in the driver.

The command runs eight cases sequentially (two directions x four providers), each through RC scripts/test-capped.sh --exec with its existing 600-second default. Inner global deadline is 550 seconds; DOM/protocol/action deadlines are 30 seconds. No cap is raised. Each case produces a sibling mesh-webui-RUN-DIRECTION-PROVIDER directory; mesh-webui-RUN/results.md combines all receipts. A failed case is retained and the next independent case runs; failed sends are never retried. No build, kill, restart, pairing, owner-window access or lifecycle operations occur.

No browser call has been executed. Live hook availability, DOM wiring, authentication, native Python availability, exporter return shape, cancellation record formats, model-switch behavior and medium-chat timing remain unproven. No product PASS/FAIL is claimed in this preparation.

## Shared browser contract

browser.mjs exports createHarness(manifest,outDir), switchPeer(page,idOrNull), selectTab(page,tabId,{mode:'chat'|'terminal'}), capture(page,label), observe(page), activity(page), waitDom(page,spec), deadline(promise,label,ms), terminalObserver, captureTerminalView(page,tabId,label).

createHarness owns browser launch/auth and returns {browser,openGateway(machineKey),close()}. openGateway returns {page,context,provenance}. Every Page.evaluate call has an outer 30-second protocol deadline. Native Playwright action/navigation/screenshot timeouts are 30 seconds. Route allowlist accepts only that context's approved gateway origin and data/blob resources. Authentication is seeded in gateway-local storage before navigation, so tokens never enter URLs, screenshot bodies or traces. No trace/HAR/auth-state file is written.

Shared output guard admits only direct children of RC target-check/evals named mesh-webui-* or mesh-terminals-*. capture stores screenshot, serving origin, selected peer, exact visible terminal-tab ID, terminal-frame visibility, tab IDs, visible text, and timestamped activity events. Plain shells select with {mode:'terminal'}. selectTab proves the visible generic terminal identity and actual selected surface; mounted hidden chat/terminal does not count.

Thompson's observer is copied into B2 terminal-capture.mjs and imported locally; refresh this copy when Thompson changes it and record its source SHA256 in retarget.diff, wrapped in the shared outer deadline. It reads actual xterm buffers, colors, cursor, normal/alternate buffer, viewport and scrollback, brackets screenshots, and fails on unavailable/truncated/changing state. It never launches, sends, switches peers, reloads or handles authentication. Optional manifest machine.terminals descriptors allow captures in the single B2 run; no terminal input/resize test is hidden there. B3 can use the same harness separately for its exact authorized terminal actions.

## Acceptance and evidence

sp_apg34f / Two DEV consoles over mesh is the contract. Four mandatory providers x seventeen checks x three views x two directions = 408 cells. B2 populates web UI cells only; desktop local/remote rows remain NOT RUN until separately attributed B1/B3 evidence arrives. A home web page is used as a concurrent status comparator; it is never called a Windows-local desktop view. MiMo is absent unless configured and assigned.

Every provider case checks peer switch, tab list, managed-by, fifteen composer turns, rendering, Working and Idle/Done, sending/progress/elapsed activity, approximately one-second convergence against the concurrent home web page, tool rows, code fences, attachments, Stop, model switch, browser-only reload/history, and Gateway self return. Screenshots include the picker menu, gateway baseline, selected tab, each send, each Working and completed turn, actions and Gateway return. Exact supplied markers link browser rendering and provider-owned turns.

Native source records are exported with tab inspection before and after to verify unchanged binding. native.py is read-only: Claude filters sessionId and sidechains in exact JSONL; Codex requires session_meta.id in the exact rollout and attaches turn_context model; Grok requires summary.info.id and a shared exact session directory for updates.jsonl and summary.json, with per-turn update model; OpenCode uses a read-only SQLite transaction and session-scoped session/message/part queries. JSONL source bytes/hash and SQLite raw scoped rows plus locators are retained. No latest-session search or global model guess is used. Export bounds are 16 MiB, subprocess output 32 MiB, and exec deadlines 30 seconds.

All remote exports and inspections go through aiderqa's RC bench lib/dc and its Windows DEV peer ID. No direct Windows CLI connection is made. Provider adapters are harness-only and do not alter delivery/ledger/PTY code, global hooks or provider plugins.

A turn PASS requires exactly one newly persisted exact user prompt, an assistant answer before the next user turn, and visible matching response. Model-switch PASS needs a distinct permitted cheap model and its value in that same new assistant turn. If the provider has only one allowed cheap model, the case records FAIL with picker/native evidence instead of spending on another model or claiming a switch. Stop requires visible active state, a Stop click, observable Idle and provider-owned cancellation evidence. Cancellation shapes are offline adapter assumptions awaiting real records; missing proof is an evidence failure, not automatically a product defect. Attachments need visible capsules and own-session input/output proof of supplied file content.

Receipts include expected/observed state, ordered steps, exact tab/session IDs, screenshots, provider evidence paths and page-error log lines. Source-only/native-infrastructure failures are distinguishable in their error text. Product defects still require collecting relevant daemon/provider log lines and exact repro in the Stage-2 report; an empty page-error array does not constitute that log proof. Repeated verdicts for the same cell remain separate; FAIL is never overwritten by later PASS. Fatal case errors remain run-failure.json and case-failures.json; missing cells are NOT RUN.

## Canonical offline gates requested

Run under Orchestrator authority in RC, using the repository's bounded runner:

    scripts/test-capped.sh --exec node --test evals/mesh-webui/model.test.mjs
    scripts/test-capped.sh --exec bash -c 'cd /Users/dev/.codeterm/runtime/agent-pads/ec518d63/b2 && python3 -m unittest -v test_evidence test_native'
    scripts/test-capped.sh --exec node --check evals/mesh-webui/browser.mjs
    scripts/test-capped.sh --exec node --check evals/mesh-webui/records.mjs
    scripts/test-capped.sh --exec node --check evals/mesh-webui/run.mjs
    scripts/test-capped.sh --exec node --check evals/mesh-webui/preflight.mjs
    scripts/test-capped.sh --exec node --check evals/mesh-webui/combine.mjs
    scripts/test-capped.sh --exec bash -n evals/mesh-webui/live.sh
    scripts/test-capped.sh --exec bash -n evals/mesh-webui/dc.sh
    scripts/test-capped.sh --exec bash -n evals/mesh-webui/collect.sh

Expected: twelve JS mechanism tests, twelve Python fixture tests, syntax checks clean. Cross-session joins, duplicate turns, missing assistant output/model, malformed native files and wrong output paths must fail closed. No test starts a browser or accesses a live DEV. Tests are authored, not executed by this Worker.

## Per-OS sampling (authored, not run)

Mac: identify aiderqa PID with lsof -nP -iTCP:7697 -sTCP:LISTEN; ps -p PID -o pid,rss,vsz,etime and lsof -nP -p PID capture memory/descriptors/sockets. PID must belong to the exact named listener.

Windows: use only Windows DEV's test shell reached through aiderqa dc --mesh. Get-NetTCPConnection -State Listen -LocalPort 7686 establishes PID; Get-Process -Id PID | Select-Object Id,WorkingSet64,PrivateMemorySize64,HandleCount,@{Name='ThreadCount';Expression={$_.Threads.Count}} captures process counts; Get-NetTCPConnection -OwningProcess PID captures sockets. B3 owns sustained leak sampling and actual renderer observations.
