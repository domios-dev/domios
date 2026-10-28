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

Use Team DEV utilities without timeouts and descriptions beginning `persistent: ...`. Reuse these deliberately for relaunches and close them at phase end. Timeout QA/build utilities are one-shot: one job, then close; never restart or reuse them.

## Candidate advance

At a quiescent boundary, record both peers' SHA/boot, friendship, gateway tab/session/nonce, policies/limits, listener state and pending message IDs. Close owned Browser profiles. Preserve pending messages and failures; do not revoke, sign out, rotate keys or replay sends to simplify an advance.

Stop both peers from the existing checkout with explicit selectors, preserving each raw exit:

```bash
cd /c/Developer/codeterm/codeterm-candidate
export DOMIOS_ACCOUNT_TARGET=qa
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

```powershell
Get-NetTCPConnection -State Listen |
  Where-Object LocalPort -in 7685,7707,7697,15007,14997,9407,9397 |
  Select-Object LocalAddress,LocalPort,OwningProcess
Get-CimInstance Win32_Process |
  Where-Object { $_.ExecutablePath -like 'C:\Developer\codeterm\codeterm-candidate\src-tauri\target\dev-instances\*' } |
  Select-Object ProcessId,ParentProcessId,CreationDate,ExecutablePath,CommandLine
```

Compare current listener owners and process creation times/paths with that instance's schema3 `dev-build-provenance.json`, native locator and attachment receipt. Verify selected data/config paths, API, client and server; inspect frontend watcher/static-server ownership separately. Exclude the owner of 7685 and foreign process trees. Recheck each identity immediately before `Stop-Process -Id <VERIFIED_NUMERIC_PID>`; stop only those verified processes, then confirm their listeners/processes absent. Uncertain identity requires owner resolution. Never kill by image name, kill an entire discovery filter, or touch production 7685.

### Build A once

Select A's four explicit variables above plus `DOMIOS_ACCOUNT_TARGET=qa`, check the full SHA, then in its persistent owner:

```bash
bash ./build.sh dev --wait --wait-build
```

Retain gateway job/log and raw exit; with `tee`, capture `${PIPESTATUS[0]}`. Require gateway release and exact A native READY before staging B. Do not compile B again or share a final Cargo target directory.

### Stage B from A and launch without Cargo

The Windows recipe below follows Witness's successful `45bd-stage-B-from-A.ps1`. Older per-peer `dev` launchers compiled twice. B must already be stopped; verify A's successful gateway/native receipt first. Set the approved full SHA, then run PowerShell:

```powershell
$ErrorActionPreference = 'Stop'
$qaRepo = 'C:/Developer/codeterm/codeterm-candidate'
$qaPin = '<FULL_CANDIDATE_SHA>'
if ((& git -C $qaRepo rev-parse HEAD).Trim() -ne $qaPin) { throw 'Candidate drift' }
$qaA = Get-Content "$env:USERPROFILE/.domios-dev-acceptance-r161/dev-build-provenance.json" -Raw | ConvertFrom-Json
if ($qaA.schema -ne 3 -or $qaA.buildSha -ne $qaPin -or $qaA.instance.name -ne 'acceptance-r161' -or [int]$qaA.instance.apiPort -ne 7707) { throw 'A provenance mismatch' }
$qaSource = [IO.Path]::GetFullPath($qaA.artifacts.resources.path).TrimEnd('\')
$qaExpected = [IO.Path]::GetFullPath("$qaRepo/src-tauri/target/dev-instances/acceptance-r161/debug").TrimEnd('\')
$qaTarget = [IO.Path]::GetFullPath("$qaRepo/src-tauri/target/dev-instances/curated/debug").TrimEnd('\')
$qaBin = "$env:USERPROFILE/.domios-dev-curated/bin"
$qaRecord = "$env:USERPROFILE/.domios-dev-curated/dev-build-provenance.json"
if ($qaSource -ne $qaExpected) { throw 'Unexpected staging root' }
if (@(Get-NetTCPConnection -State Listen -LocalPort 7697 -ErrorAction SilentlyContinue).Count) { throw 'B listening' }
$qaBusy = Get-CimInstance Win32_Process | Where-Object {
  $_.ExecutablePath -and ($_.ExecutablePath.StartsWith($qaTarget+'\',[StringComparison]::OrdinalIgnoreCase) -or
    $_.ExecutablePath.StartsWith([IO.Path]::GetFullPath($qaBin)+'\',[StringComparison]::OrdinalIgnoreCase))
}
if (@($qaBusy).Count) { throw 'B artifacts in use' }
New-Item -ItemType Directory -Force -Path $qaTarget,$qaBin | Out-Null
foreach ($qaRoot in @($qaSource,$qaTarget,$qaBin)) {
  if ((Get-Item -LiteralPath $qaRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Root link refused' }
}
foreach ($qaName in @('cli','skills','conpty','dist','domios-server')) {
  $qaTo = [IO.Path]::GetFullPath((Join-Path $qaTarget $qaName))
  if (-not $qaTo.StartsWith($qaTarget+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path' }
  if (Test-Path -LiteralPath $qaTo) {
    if ((Get-Item -LiteralPath $qaTo).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Artifact link refused' }
    Remove-Item -LiteralPath $qaTo -Recurse -Force
  }
  Copy-Item -LiteralPath (Join-Path $qaSource $qaName) -Destination $qaTo -Recurse -Force
}
foreach ($qaName in @('domios-dev-app.exe','server-component.json','LICENSE','NOTICE')) {
  Copy-Item -LiteralPath (Join-Path $qaSource $qaName) -Destination (Join-Path $qaTarget $qaName) -Force
}
foreach ($qaName in @('codeterm.exe','domios.exe')) {
  Copy-Item -LiteralPath $qaA.artifacts.cli.path -Destination (Join-Path $qaBin $qaName) -Force
}
# Preserve existing B provenance in the private run evidence before replacing it.
Push-Location $qaRepo
try {
  $qaArgs = @('--record',$qaRecord,'--build-sha',$qaPin,
    '--app',"$qaTarget/domios-dev-app.exe",'--cli',"$qaBin/codeterm.exe",
    '--frontend',"$qaTarget/dist",'--instance','curated','--api-port','7697',
    '--vite-port','14997','--cdp-port','9397','--identifier','com.domios.dev.curated')
  & node scripts/dev-prep.mjs --write-provenance @qaArgs
  if ($LASTEXITCODE -ne 0) { throw 'Provenance write refused' }
  & node scripts/dev-prep.mjs --check-provenance @qaArgs
  if ($LASTEXITCODE -ne 0) { throw 'Provenance check refused' }
} finally { Pop-Location }
$qaB = Get-Content -LiteralPath $qaRecord -Raw | ConvertFrom-Json
foreach ($qaName in @('app','cli','frontend','server','inventory','resources')) {
  if ($qaB.artifacts.$qaName.sha256 -ne $qaA.artifacts.$qaName.sha256 -or
      $qaB.artifacts.$qaName.bytes -ne $qaA.artifacts.$qaName.bytes) { throw "Artifact mismatch: $qaName" }
}
```

Retain all six artifact comparisons and exits. This official writer records a verified same-source transfer; never use it to bypass stale source or changed bytes, or hand-edit its JSON. Select B's four explicit variables plus `DOMIOS_ACCOUNT_TARGET=qa`, then:

```bash
bash ./build.sh dev-prebuilt --wait
```

## Qualification and frontend recovery

Independently require on both peers before new QA sends:

- Authenticated health reports the full candidate SHA, selected instance/API and fresh boot ID after restart.
- Frontend `/` and its referenced JavaScript return 200; `/build-identity.json` matches the candidate.
- `DOMIOS_DEV_ATTACHMENT` protocol 2 names live client/server PIDs, the selected API address and server epoch; native READY agrees with the actual listener. Preserve raw launcher exit and receipt. Health alone does not prove native attachment.
- Account is signed in, not pending, Firebase and privately matches the intended alias; retain equality booleans only.
- Restored friendship/policies/limits and exact gateway session/binding nonce are correct; concierge has its own readiness attestation. Record listener health/errors separately from admission; reconnecting is not a healthy-stream PASS.

Select `CODETERM_INSTANCE` and `CODETERM_API_PORT` before sourcing `evals/_lib/lib.sh`. Benches use only `dc`, `dev_get`, `dev_post` or `dev_run`; `evals/qa/tools/dev-session.sh` records actions, UTC and raw exits. Host tab IDs/credentials are not DEV identities. Actual General/concierge actors use their own inherited launch credentials. Use the built-in Browser plugin for UI work.

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

<!-- revision: 1 -->
