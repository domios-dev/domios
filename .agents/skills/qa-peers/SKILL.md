---
name: qa-peers
description: Advance, qualify, recover and clean up the two Windows named DEV peers used for Neighbors acceptance on a pinned candidate.
---

# Neighbors QA peers

Use only with the owner's candidate-advance and two-QA-account grants. This procedure grants no production, account/key, external-message or additional test actions. Read [dev-run](../dev-run/SKILL.md), [dev-testing](../dev-testing/SKILL.md), [testing](../testing/SKILL.md) and [CUR-048](../../../evals/qa/cases/CUR-048-neighbors-encrypted-delivery/case.md). Commands below target Windows; shell blocks use Git Bash unless marked PowerShell.

## Topology

Both peers use the detached worktree `C:/Developer/codeterm/codeterm-candidate`. Freeze its full SHA throughout the build and QA phase.

| Peer | Instance | API | Frontend | CDP | QA account alias |
| --- | --- | --- | --- | --- | --- |
| A | acceptance-r161 | 7707 | 15007 | 9407 | Buyer |
| B | curated | 7697 | 14997 | 9397 | free (QA-role staff Peer) |

Both select `DOMIOS_ACCOUNT_TARGET=qa`. State remains separate in `~/.domios-dev-acceptance-r161` and `~/.domios-dev-curated`. Never copy configuration, accounts, databases, receiving keys or user data between them. QA admin is a separate refusal identity; `domios-qa-unpaid` is the separately authorized non-staff unpaid account. Buyer/free staff access does not prove subscription or unpaid limits. Retrieve credentials privately from the authorized store; record aliases and equality booleans only, never emails, UIDs, secrets or authentication URLs.

Both persistent DEV owners must export `CODETERM_DEV_WATCH=0` before `dev` or
`dev-prebuilt`. This skips only the bundle watcher; static serving, provenance
validation and native attachment remain required. A watcher rebuild mutates the
shared artifact tree and invalidates the other peer's provenance, even at unchanged
source. Keep the candidate frontend immutable throughout QA; rebuild and restage
through the supported flow after any change, never recapture changed live bytes
to bypass a provenance refusal.

Use Team DEV utilities without timeouts and descriptions beginning `persistent: ...`. Reuse these deliberately for relaunches and close them at phase end. Timeout QA/build utilities are one-shot: one job, then close; never restart or reuse them.

## Candidate advance

At a quiescent boundary, record both peers' SHA/boot, friendship, gateway tab/session/nonce, policies/limits, listener state and pending message IDs. Close owned Browser profiles. Preserve pending messages and failures; do not revoke, sign out, rotate keys or replay sends to simplify an advance.

Stop both peers from the existing checkout with explicit selectors, preserving each raw exit:

```bash
cd /c/Developer/codeterm/codeterm-candidate
export DOMIOS_ACCOUNT_TARGET=qa
export CODETERM_DEV_WATCH=0
export CODETERM_DEV_INSTANCE=acceptance-r161 CODETERM_DEV_API_PORT=7707
export CODETERM_DEV_VITE_PORT=15007 CODETERM_DEV_CDP_PORT=9407
bash ./build.sh kill
# Separately, for B:
export CODETERM_DEV_INSTANCE=curated CODETERM_DEV_API_PORT=7697
export CODETERM_DEV_VITE_PORT=14997 CODETERM_DEV_CDP_PORT=9397
bash ./build.sh kill
```

After both are proven down, require a clean checkout, run `git checkout --detach <FULL_CANDIDATE_SHA>` and compare `git rev-parse HEAD` with that full SHA. Never reset dirty work or move the candidate while compiling/testing.

### Old-launch shutdown refusal

Preserve `DEV process changed before shutdown` or `Uncertain DEV instance identity; refusing shutdown`; do not retry or weaken the guard. The authorized DEV owner must independently verify the current processes, not reuse old PIDs:

Both peers run from the same executables, so an image path never identifies a peer. Start from each peer's own API listener and its own provenance and data root:

```powershell
$qaPeers = @(@{ Name = 'acceptance-r161'; Port = 7707 }, @{ Name = 'curated'; Port = 7697 })
foreach ($qaPeer in $qaPeers) {
  $qaRecord = Get-Content "$env:USERPROFILE/.domios-dev-$($qaPeer.Name)/dev-build-provenance.json" -Raw | ConvertFrom-Json
  $qaHealth = try { Invoke-RestMethod "http://127.0.0.1:$($qaPeer.Port)/api/health" -TimeoutSec 2 } catch { $null }
  $qaOwner = @(Get-NetTCPConnection -State Listen -LocalPort $qaPeer.Port -ErrorAction SilentlyContinue).OwningProcess
  [pscustomobject]@{ Peer = $qaPeer.Name; RecordPort = $qaRecord.instance.apiPort; Server = $qaRecord.artifacts.server.path
    HealthInstance = $qaHealth.instance.name; ListenerPid = $qaOwner }
}
Get-NetTCPConnection -State Listen |
  Where-Object LocalPort -in 7685,7707,7697,15007,14997,9407,9397 |
  Select-Object LocalAddress,LocalPort,OwningProcess
```

A matching health reply proves a running peer; failed, unhealthy or mismatched health never proves it stopped. Verify retained client/server process creation identities and scan the target for surviving processes, including a client without a server. Unknown liveness refuses adoption and rebuilding. Take that listener's process, its ancestors and descendants from `Get-CimInstance Win32_Process` (`ProcessId`, `ParentProcessId`, `CreationDate`), and the client named by the peer's native locator and attachment receipt. Verify selected data/config paths, API, client and server; inspect frontend watcher/static-server ownership separately. Exclude the owner of 7685, the other peer's tree and foreign process trees. Recheck each identity immediately before `Stop-Process -Id <VERIFIED_NUMERIC_PID>`; stop only those verified processes, then confirm their listeners/processes absent. Uncertain identity requires owner resolution. Never kill by image name or image directory, kill an entire discovery filter, or touch production 7685.

### Build A once

Select A's four explicit variables above plus `DOMIOS_ACCOUNT_TARGET=qa`, check the full SHA, then in its persistent owner:

```bash
export CODETERM_DEV_WATCH=0
bash ./build.sh dev --wait --wait-build
```

Retain gateway job/log and raw exit; with `tee`, capture `${PIPESTATUS[0]}`. Require gateway release and exact A native READY before starting B. Both peers share the candidate's one DEV target `src-tauri/target/named-dev`; never compile B separately.

`dev` and `dev-server` refuse to compile while any DEV started from that target is running. The refusal names each running peer and its exact `kill` command. Windows locks a running executable, so a rebuild under a live peer cannot replace it. Stop both peers first; never work around the refusal with another instance name or target directory.

### Start B from A's build without Cargo

A's `dev` wrote the shared build record `src-tauri/target/named-dev/dev-build-provenance.json`. B must already be stopped. Select B's four explicit variables plus `DOMIOS_ACCOUNT_TARGET=qa`, then:

```bash
export CODETERM_DEV_WATCH=0
bash ./build.sh dev-prebuilt --wait
```

Without a current record of its own, B's `dev-prebuilt` validates the shared record against HEAD, the build inputs and every artifact hash. It then installs the built CLI into `~/.domios-dev-curated/bin`, writes B's own provenance (B's name, ports and identifier) and launches from the shared artifacts. A stale or changed shared build is a refusal: rebuild through A's `dev` after stopping both peers. Never copy artifacts or hand-write provenance. Retain the `[named-dev] instance 'curated' adopted the shared build` line and the raw exit.

The first named `dev` after this layout landed renames one warm legacy `src-tauri/target/dev-instances/<name>` to `named-dev` (newest instance record wins; others stay). It refuses while a DEV still runs from that legacy target.

## Qualification and frontend recovery

Independently require on both peers before new QA sends:

- Authenticated health reports the full candidate SHA, selected instance/API and fresh boot ID after restart.
- Frontend `/` and its referenced JavaScript return 200; `/build-identity.json` matches the candidate.
- `DOMIOS_DEV_ATTACHMENT` protocol 2 names live client/server PIDs, the selected API address and server epoch; native READY agrees with the actual listener. Preserve raw launcher exit and receipt. Health alone does not prove native attachment.
- Account is signed in, not pending, Firebase and privately matches the intended alias; retain equality booleans only.
- Restored friendship/policies/limits and exact gateway session/binding nonce are correct; concierge has its own readiness attestation. Record listener health/errors separately from admission; reconnecting is not a healthy-stream PASS.

Select `CODETERM_INSTANCE` and `CODETERM_API_PORT` before sourcing `evals/_lib/lib.sh`. Benches use only `dc`, `dev_get`, `dev_post` or `dev_run`; `evals/qa/tools/dev-session.sh` records actions, UTC and raw exits. Host tab IDs/credentials are not DEV identities. Actual General/concierge actors use their own inherited launch credentials. Use the built-in Browser plugin for UI work.

### Automated QA sign-in

The hosted QA sign-in page runs App Check (reCAPTCHA Enterprise) before sign-in, which fails in an automated browser ("Browser verification could not be completed"). Never bypass it interactively. The supported path is QA project only: a registered App Check debug token in the secret store (ask the backend owner for its name; never print it). Read it through stdin and inject `self.FIREBASE_APPCHECK_DEBUG_TOKEN` before page scripts run (CDP `Page.addScriptToEvaluateOnNewDocument` or a Browser init script), start sign-in from the peer, type the QA account credentials from the secret store through stdin, then remove the script or close the tab. Token values never go into argv, chat, logs or this file.

For a broken frontend on an unchanged build, preserve diagnostics, use supported named shutdown, then the same named `dev-prebuilt --wait` without recompiling. Requalify frontend and native attachment. A provenance mismatch remains a refusal. Windows single-tab watcher/static-server processes detach from the utility job; their instance pidfiles are JSON with `pid`, `created` (FILETIME) and `executable`. Supported cleanup verifies all three and pins process handles. Stale, mismatched or bare-PID records are skipped, never treated as ownership. Do not weaken cleanup or stop another peer's frontend.

## Acceptance order and evidence

Follow CUR-048 and the selected release rows; this order does not expand the grant:

1. Qualify both peers/accounts and backend provenance. For an authorized clean cycle, capture old relationship revoke/history state, submit A's existing-account request once, accept on B, and complete both real General interviews. Require automatically created nested concierges.
2. Exchange permitted public-scope questions/replies both directions. Join exact own-session admission to independent redacted backend ACK/inbox-deletion evidence. Never replay uncertain sends or answer an answer.
3. Run selected permission/harness controls, outside-scope refusals, tab/memory/machine access, capabilities, size/rate limits, files, automatic delivery, untouched background/restart wake, pause/resume and history. Restore temporary policy/limits at quiescent boundaries. Preserve initial failures separately from fixture corrections.
4. Perform explicitly granted receiving-machine and normal Mesh drills, then sign-out/account-switch and non-staff unpaid enforcement last. Standard DEV 7686 requires its separate drill grant; never touch 7685. Unsupported/native-device rows remain NOT RUN.

Raw artifacts and ownership manifests go under `target-check/evals/qa/<stamp>/<CUR-id>/` with `results.json` following `evals/qa/results.schema.json`. Generate matrices through `evals/qa/tools/catalogue.mjs matrix`; durable release summaries go in `evals/qa/matrix/<version>.md` and the acceptance ledger (1.16.1 uses `1.16.1-acceptance.md`). Record candidate/backend provenance, UTC, exact actions/raw exits, IDs, far-end evidence and cleanup. Keep secrets/private backups out of git. Driver exit 0 alone is not PASS; distinguish FAIL, NOT RUN, BLOCKED and OWNER-SKIPPED.

## End the phase

Restore owned temporary policies/limits; reconcile jobs, tabs, files and pending messages against the manifest. Close only owned Browser profiles via the Browser plugin and close completed timeout utilities immediately. When the owner ends the peers' phase, stop both through explicit named `build.sh kill`, confirm their selected processes/listeners absent, then close persistent DEV/controller utilities. Retain the shared candidate checkout/account state unless removal is explicitly requested. Never delete unrelated tabs or another worker's evidence.

<!-- revision: 4 -->
