# Shared B2/B3 renderer capture interface

Offline source preparation only. Euler reports canonical 12 tests passed and sample/capture shell syntax clean; no Worker gate executed. Euler relayed owner authorization for headless isolated Playwright contexts restricted to aiderqa/Windows DEV, no DEV lifecycle actions. Still no GO live.

B2 owns the single browser driver: browser/context launch, allowed origins, authentication, titlebar peer picker, tab selection, deadlines, failure captures, teardown. B3 adds terminal-capture.mjs as an importable observer on B2's existing Page. It launches/connects no browser and performs no sends, reloads or lifecycle actions. Never copy B2 browser connection logic into B3.

Exports: readTerminalState (serializable DOM evaluator, reusable through CDP Runtime.evaluate), armTerminalEvents(page, tabId), waitTerminalEvent(page, probeId, {type, after, deadlineMs}), captureTerminal(page, {tabId, probeId, directory, provenance}), releaseTerminalEvents(page, probeId).

Source-backed hook: src/hooks/terminal/useXterm.ts sets host.__codetermTerminal only for viteEnv.DEV. Root identity is terminal-tab[data-tab-id], host terminal-xterm-host. Composite mesh tab identities must be passed verbatim from DOM. No xterm hook means capture fails; accessibility-mode DOM text would change truecolor behavior and is not used as a fallback.

Read state captures actual xterm buffer text and cell colors/width/rendition, rows/cols, cursor coordinates, alternate/normal buffer, viewport/history and wrapping. Cursor hidden is internal diagnostic access; if null, use screenshot + read-only CDP inspection to resolve, never invent visibility. 10000-line cap fails if truncated. Screenshot is full page to preserve peer/tab provenance. State before/after brackets screenshot; a changed state fails and preserves both states/screenshots. onRender is evidence of an event, not turn readiness or success. Await a render event after the action and verify the required result separately. B2 must bound page.evaluate/session calls externally too, because a hung CDP connection cannot be bounded by a renderer AbortSignal. Failure deadlines are the only timers; no delays or polling.

Local view for Windows-home remains the Windows home daemon formatted screen as baseline, supplemented by actual home screenshot only if supplied by Orchestrator. Windows-home remote and web rendered terminal evidence is captured through the Mac desktop/web views over mesh as directed by Euler. A Mac-rendered view must never be relabelled Windows-local renderer evidence.

Suggested B2 usage, invoked only after GO live:

```js
const probeId = await armTerminalEvents(page, exactDomTabId);
try {
  const before = await captureTerminal(page, { tabId: exactDomTabId, probeId, directory: beforeDir, provenance });
  await performAlreadyAuthorizedSurfaceAction();
  await waitTerminalEvent(page, probeId, { type: 'render', after: before.revision, deadlineMs: 30000 });
  const after = await captureTerminal(page, { tabId: exactDomTabId, probeId, directory: afterDir, provenance });
  // Inspector compares the required step outcome and provider own-session records.
} finally {
  await releaseTerminalEvents(page, probeId);
}
```

Mac native desktop screenshots use the existing RC script exactly:

```bash
node /Users/kandrey/Developer/codeterm-worktrees/rc-mac/scripts/dev-screenshot.mjs RC_RUN/captures/STEP/mac-desktop.png '' --api-port 7697
```

Wrap in the Orchestrator's bounded runner; it has no internal request deadline. Read terminal state using the same readTerminalState evaluator through the existing CDP connection supplied by B2/Orchestrator. Remote Mac desktop selection is task-owned and explicitly identifies Windows DEV tab. No owner BLACKSTAR window is touched. The existing screenshot script is source-inspected, not run.

Deferred adapter check: node --check evals/mesh-webui/terminal-capture.mjs. Live hook availability, actual renderer provenance, event timing, screenshot coherence and xterm internal cursor shape remain unproven.

B2 confirmed shared module path evals/mesh-webui/browser.mjs, forthcoming exports createHarness(manifest, outDir), switchPeer(page, peerIdOrNull), selectTab(page, tabId), capture(page, label). openGateway(machineKey) returns page/context/provenance; B3 consumes that exact Page. B2 tab selection currently proves tab-session-chat; B3 needs generic shell selection and then Terminal mode. Source-backed switch is tab-mode-switch[data-tab-id] with child [data-value="terminal"]. Assert matching terminal-frame[data-terminal-visible="true"] before reading, not merely mounted terminal-tab. Ask B2 to expose this selection option once, rather than clone it.

Additional authored offline tests: terminal-capture.test.mjs (Unicode/rendition and alternate serialization; hidden/ambiguous tab rejection; changing capture retains FAIL evidence). Deferred command: node --test evals/mesh-webui/terminal-capture.test.mjs. No Worker execution claimed.

## Shared-driver executable capture entry

New capture-web.mjs imports B2 browser.mjs; it creates no separate connection/auth driver. It selects only manifest-owned provider/shell fixtures and captures web Terminal mode, preserving real provenance and return through Gateway self. It does not label web images as native desktop evidence or submit/interrupt anything. This is a before/after capture entry, not the full live behavior matrix executor. Both GO env flags are required.

Deferred syntax check:

```bash
node --check evals/mesh-webui/capture-web.mjs
```

Exact live command after GO, with assigned manifest and case file:

```bash
B2_GO_LIVE=yes B3_GO_LIVE=yes node evals/mesh-webui/capture-web.mjs MANIFEST_JSON /Users/kandrey/Developer/codeterm-worktrees/rc-mac/target-check/evals/mesh-terminals-B3-RUN CASES_JSON
```

CASES_JSON is a nonempty array of {id, gateway:'mac'|'windows', home:'mac'|'windows', view:'web', tabId}. Each tab must be in manifest[home].tabs or optional manifest[home].shells. Distinct ids are required. Origins/exact peer ids/token files stay B2 manifest-owned. Do not supply shell snippets or input payloads; only capture identities. New directories/observation evidence preserve originals. Orchestrator outer process deadline still required.

Shared-driver adjustments requested from Engelbart: createHarness allow approved mesh-terminals-* alongside mesh-webui-*; generic visible terminal-tab provenance fallback when chat is hidden. Until applied, capture-web is authored but blocked by the shared B2 path guard. Windows-home native capture is now an explicit planned Orchestrator Windows DEV-only BLACKSTAR shell/CDP route, supplied at GO.

Canonical executed evidence supplied by Euler:
- /Users/kandrey/Developer/codeterm-worktrees/rc-mac/target-check/evals/mesh-phaseb-offline/b3-offline.txt: original 12 tests pass and shell syntax clean per Euler.
- /Users/kandrey/Developer/codeterm-worktrees/rc-mac/target-check/evals/mesh-phaseb-offline/b3-capture-offline.txt: terminal-capture.mjs syntax check passed; 3 Node tests passed, 0 failed per Euler.

No live matrix results exist yet. capture-web.mjs is new and has no executed syntax/live proof.

Shared integration source verification complete: Engelbart's model.validateOutput now accepts only direct RC eval children mesh-webui-* or mesh-terminals-*; browser.capture includes visible terminal-tab selectedTabId and terminalVisible. B3 capture-web imports that same output guard rather than maintaining duplicate path policy. Prior shared path/provenance blockers are resolved in source; runtime remains unexecuted pending GO. The latest capture-web revision still needs the deferred syntax check.

Current respawn wiring: Engelbart supplied ec518d63/b2/browser.mjs and model.mjs. B3 imports those shared modules; browser.mjs imports this pad’s terminal-capture.mjs. Inspected exports and Windows origin guard http://100.84.159.37:7686. No syntax/runtime gate or browser connection executed. Await Euler GO live, exact peer id and populated manifest.


## Missing DEV xterm and wrong fixture provenance

Symptom: phase-B web terminal captures fail with `Cannot arm missing DEV xterm`
(or `DEV xterm instance unavailable`) although the desktop DEV has the hook.
The DEV daemon serves `CODETERM_WEB_DIST`; its watcher previously ran Vite's
production build defaults. `CODETERM_FRONTEND_DEV` supplies the DEV titlebar
label only. `build.sh start_web_watch` now supplies command-local
`NODE_ENV=development` and `--mode development` for both Windows and Unix.
Both are intentional: Vite mode alone does not override production `NODE_ENV`
for a build. Production `pnpm build` and release preparation are unchanged.

Recovery belongs to the Orchestrator after merge: restart the affected DEV
through its scoped launcher, confirm the watcher completed a fresh bundle,
reload its web UI, select the manifest fixture in Terminal mode, and inspect
that exact root's xterm host for `__codetermTerminal`. Then rerun the approved
capture in a fresh run directory. Preserve original failure evidence; no
DOM-text substitute or production hook is allowed. See the dev-run skill's
all-platform troubleshooting entry for lifecycle details.

A mounted hidden General Agent root can still have client rectangles and
previously supplied the wrong `selectedTabId`. Shared `capture(page, label,
{tabId})` now evaluates `readSelectedTab` from `provenance.mjs`, matching the
requested `data-tab-id` verbatim and requiring visible CSS/opacity plus positive
geometry. Missing, hidden, zero-size or ambiguous matches throw. Successful
`selectTab` records the requested identity for subsequent B2 captures; peer
switch completion clears it. A gateway-return capture without a requested tab
requires exactly one visible root. B3 and `captureTerminalView` pass the
fixture identity explicitly. `terminalVisible` comes from that same root and
can be false for a chat capture; terminal capture separately requires true.

Deferred canonical oracles (no browser launch or Rust compile):

```bash
node --test evals/mesh-webui/*.test.mjs
node --test scripts/tests/dev-wait.test.mjs
```

The provenance fixtures include a hidden `46137477` root before the requested
fixture, exact mesh identities and rejection paths. The existing dev-wait
suite now guards both platform watcher commands and the unchanged release
frontend command. Runtime acceptance still requires the rebuilt DEV browser's
hook and capture metadata `selectedTabId` equal to the requested fixture, with
`terminalVisible` true for the terminal leg, followed by coherent buffer and
screenshot evidence under the existing capture contract.
