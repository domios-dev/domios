# Startup evidence scenario index

These commands promote capture pieces from Keel's live2/live3 diagnostics. They
do not promote the scratch navigation runners or their incomplete receipts.
`evidence.sh` pins the default DEV connection through `_lib/lib.sh` and
`dev_run`. No command below calls a raw CLI against DEV. Node 22+ is required.
Reserve active DEV work with its owner; the Worker only runs pure tests.

Replace every uppercase argument with an explicitly selected fixture. `SHA` is
the full expected binary commit, not an asset URL fingerprint. Output directories
must be fresh. Browser CDP must be the owned Browser's loopback endpoint, with
the exact page target ID obtained through the Browser plugin. It is separate
from the native DEV window's CDP endpoint. The target URL must be DEV port 7686.

| Scenario / purpose | Versioned command | Fixture | Real far-end oracle | Artifacts / schema | Known gaps |
|---|---|---|---|---|---|
| Served and actually loaded bundle identity | `bash evals/startup-matrix/evidence.sh browser --expect-build SHA --out OUT --cdp http://127.0.0.1:PORT --target TARGET --tab TAB --reload` | Existing owned DEV Browser page and mounted PTY tab | CDP `Debugger.getScriptSource` content hash compared with independently fetched served asset bytes beside binary SHA | `browser.json.assets`, `.index`, `loaded-HASH.js`, `served-HASH.js`, `served-index-HASH.html`; [schema](evidence.schema.json) | JS bundle capture, not a startup or reply PASS. CSS sources are not captured. A hash mismatch is evidence, not silently normalized. Script URLs alone never prove bytes. |
| Raw SSE and every mounted xterm buffer | Same browser command | Same; retained hidden terminals remain mounted | Actual CDP `Network.streamResourceContent` bytes, per request; active/normal/alternate buffers retain tab ID and ancestor visibility | `output-N.sse`, `browser.json.streams`, `.before`, `.after`; [schema](evidence.schema.json) | Only streams opened after arming and the owned page's mounted terminals. No reconstructed frames. No reference-byte integrity or hidden phone claim. Buffer contents can lag the first wire data; this is a capture boundary, not a flush/readiness oracle. |
| Startup origins and breakdown | `bash evals/startup-matrix/evidence.sh phases PHASES_JSON STATE_JSON OUT_JSON [RAW_LOG BEGIN_US END_US]` | Existing `fresh-phases.mjs` receipt plus exact final/later mux state; optional explicit inclusive epoch-us raw log window | Exact tab/PTY boot marks and recorded tab creation stamp; nulls expose missing sources | `phase-origins` JSON rows with value/unit/source for CLI begin, socket receive/dispatch/bind, utility creation, PtyStarting, AgentReady; [schema](evidence.schema.json) | CLI begin is wrapper time, not user click. Later state does not timestamp convergence. Missing phases/provider env/build remain unknown. Restored tabs lacking CLI origin remain incomplete. |
| Unmodified shipped web diagnostics | Same browser command | Same owned Browser; reload explicitly authorized | Natural request from shipped page, HTTP 200 **and** parsed `{ok:true}` response | `browser.json.diagnostics` sanitized request fields/status/response; [schema](evidence.schema.json) | No injected hook or manufactured failure. Acknowledgement proves diagnostic delivery only; no complete UI readiness or Safari/iPad claim. |
| Exact-session recovery and no replay | `bash evals/startup-matrix/evidence.sh recovery --expect-build SHA --out OUT --fixture FIXTURE_JSON --force-exit` | Windows resumed PTY process, existing exact own-session user probe, PID+creation and tab/session/nonce fixture | A new PTY AgentReady; same session authority; changed process PID+creation identity; exactly unchanged ordered own user-turn IDs before/after | `recovery.json`, `before-exit.json`, `process-before.json`, `process-exited.json`, `process-after.json`, bounded `recovery.log`; [schema](evidence.schema.json) | Explicitly terminates only the verified DEV-descendant resumed provider process. Windows ancestry adapter only; other platforms refuse. No resend. Comparison covers returned transcript window, not all historical turns. |

The browser command reloads **only** the selected existing page. It does not
click sidebars, return to another tab, restore a draft, create pages, reset
counters, or inject diagnostic code. It waits for a natural diagnostic response,
raw stream bytes and the selected mounted terminal, then snapshots all mounted
terminals. Existing draft/session behavior needs a separate journey oracle.
Console metadata retains type/time only; raw message text is omitted.

## No-effect planning and fixture validation

Keel owns the single DEV dry run. After review, run one selected command:

```bash
bash evals/startup-matrix/evidence.sh browser --expect-build SHA --out OUT --cdp http://127.0.0.1:PORT --target TARGET --tab TAB --dry-run
bash evals/startup-matrix/evidence.sh recovery --expect-build SHA --out OUT --fixture FIXTURE_JSON --dry-run
```

Dry-run reads health, verifies exact binary SHA and a single DEV listener's
PID+kernel creation time, validates the selected fixture, and (for Browser)
reads CDP targets and opens/closes its protocol connection. Recovery additionally
reads current tab/session/transcript and process ancestry. Ordinary request
telemetry may be created. It sends no agent input and changes no lifecycle,
page, configuration, manifest, counters or process. Output says `mode: dry-run`;
the offline contract returns `validated`, never PASS or capture coverage.
Browser receipts retain the selected `fixture.tabId` and its session/nonce/PTY/
boot-stage authority snapshot independently of the CDP target. This does not
claim the selected tab was displayed. A missing fixture identity is incomplete.
On Windows PowerShell, invoke Git Bash explicitly (for example
`& 'C:/Program Files/Git/bin/bash.exe' evals/startup-matrix/evidence.sh ...`);
bare `bash` may resolve to WSL without a usable `/bin/bash`.

Recovery fixture (all values must describe the current own DEV fixture):

```json
{
  "tab": "TAB",
  "session": "EXACT_SESSION",
  "nonce": "CURRENT_NONCE",
  "marker": "UNIQUE_EXISTING_USER_PROBE",
  "process": { "pid": 1234, "created": "2026-10-16T00:00:00.0000000Z" }
}
```

Obtain process identity with the same read-only adapter, under the wrapper:

```bash
source evals/_lib/lib.sh
dev_run powershell.exe -NoProfile -File evals/startup-matrix/recovery-process.ps1 -Session EXACT_SESSION -OutputFile PROCESS_JSON
```

The force-exit adapter rechecks exact `--resume SESSION`, DEV ancestry, single
candidate, PID and kernel creation time immediately before termination. It
does not enumerate or terminate by provider name alone. Do not run it against a
fresh process without `--resume`; that is a different scenario.

Fresh phase collection remains the existing command (active, token spending):

```bash
source evals/_lib/lib.sh
dev_run node evals/startup-matrix/fresh-phases.mjs SHA OUT WORKSPACE shell,claude,codex
```

Use its `phases.json` plus a separately saved exact state for the origins
converter. It now retains `[boot-request] bound_at_us` as well as boot phases.
Optional raw log input is bounded to 16 MiB and must specify the actual window;
there is no implicit 180-second window or provider fallback to `shell`.

## Receipt contract and bounds

All receipts use schema 1 and an explicit `scenario`. Browser/recovery retain
collector `status: completed|incomplete`; **completed means collection settled**,
not a product PASS. Phase origins use `captured|incomplete`. Missing evidence
is incomplete even if another field claims success. Dry-run is only validated.
Only the recovery own-session verdict can return `pass`, after all listed
authority/process/user-turn observations are present. Browser returns `captured`
and a narrowly scoped diagnostic verdict; phase origins return `captured`.

```bash
node evals/startup-matrix/evidence-contract.mjs OUT/browser.json
node evals/startup-matrix/evidence-contract.mjs OUT/recovery.json
node evals/startup-matrix/evidence-contract.mjs OUT_JSON
```

Contract exits: 0 validated/captured/passing recovery; 1 incomplete; 3 observed
failure. The CLI checks referenced Browser asset/SSE file lengths and SHA-256,
so a JSON receipt without its byte artifacts cannot validate.

The live capture has a 120-second **failure deadline**, never a success timer.
Recovery subscribes before forcing exit. The helper owns no output pipes and
settles on child `exit`, independently of descendant-held pipe `close`; both
that exit and the exact new-PTY AgentReady must precede the final read. The
microtask regression exercises exit without close; it does not establish the
cause of the historical hang. The outer collector persists incomplete evidence
and closes owned resources even when an operation ignores cancellation.

Active receipts recheck the binary SHA and original DEV listener PID+creation
identity at the far end. A changed daemon cannot pass as provider recovery.

Raw bounds: SSE 16 MiB aggregate, loaded+served JS assets 64 MiB, served HTML 4 MiB, each xterm buffer
1 MiB and all xterm text 8 MiB, recovery log 16 MiB aggregate/8 MiB per read,
console 4096 records, diagnostics 256 requests. Exceeding a bound is incomplete;
no truncated bytes are advertised as complete. Shareable URL metadata removes
credentials, query and fragment. Raw SSE, source files, xterm text and logs may
contain user content or secrets; keep them local and inspect/redact copies
before sharing. Never rewrite raw bytes in place and retain their original hash.

## Provenance and excluded receipts

Extracted from Keel's `live3-q4-capture.mjs`, `q4-evidence-functions.mjs`,
`live3-phase-read.mjs`, `live3-phase-table.mjs`, `live3-origins.mjs`,
`live3-r4.mjs`, `live3-restored-recovery.mjs`, `live3-restored-exit.ps1` and
`live3-r2-classify.mjs`. Paths, build IDs and fixtures were ephemeral and are
intentionally absent from the commands above. Pure fixture data is versioned at
`tests/fixtures/startup-evidence.json`; pure tests cover each capture's decisions.
Live wiring remains unexecuted until Keel's authorized run.

Historical live3 Q4 terminal evidence was positive but the whole runner exited
1 on later navigation. Historical recovery was interrupted with no final
`recovery.json`; later read-only reconciliation saw recovery and unchanged user
turn IDs. Neither is a completed passing run of these promoted commands.
The old settlement root cause remains unproven.

Excluded as ready runners: `q4-browser.mjs`, unscoped `q5-divider.mjs`,
`chat-*-wake.mjs`, injected `diagnostic-live-preflight.mjs` /
`web-diagnostic-hook.mjs`, and manifest/CAS `s2-manifest-mode.mjs` shell timing.
Pager output, virtual viewport, frozen tab and source URL lists are never
relabeled as byte integrity, real mobile visibility or content identity proof.
