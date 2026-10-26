import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

test('DEV process safety accepts multiple addresses of one owner and refuses absent or competing owners', { skip: process.platform !== 'win32' }, () => {
  const command = `
    $ErrorActionPreference = 'Stop'
    . $env:PHASE_OWNER_MODULE
    $owner = Resolve-DevListenerOwner -Listeners @(
      [pscustomobject]@{ LocalAddress='127.0.0.1'; OwningProcess=72596 },
      [pscustomobject]@{ LocalAddress='100.64.10.1'; OwningProcess=72596 }
    )
    if ($owner -ne 72596) { throw 'Wrong owner' }
    $emptyRefused = $false
    try { Resolve-DevListenerOwner -Listeners @() } catch { $emptyRefused = $true }
    if (-not $emptyRefused) { throw 'Absent owner accepted' }
    $competingRefused = $false
    try { Resolve-DevListenerOwner -Listeners @(
      [pscustomobject]@{ OwningProcess=72596 },
      [pscustomobject]@{ OwningProcess=6424 }
    ) } catch { $competingRefused = $true }
    if (-not $competingRefused) { throw 'Competing owners accepted' }
    $zeroRefused = $false
    try { Resolve-DevListenerOwner -Listeners @([pscustomobject]@{ OwningProcess=0 }) } catch { $zeroRefused = $true }
    if (-not $zeroRefused) { throw 'Invalid owner accepted' }
    Write-Output 'PASS'
  `;
  const output = execFileSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', command], {
    encoding: 'utf8', windowsHide: true,
    env: { ...process.env, PHASE_OWNER_MODULE: fileURLToPath(new URL('../../evals/startup-matrix/process-owner.ps1', import.meta.url)) },
  });
  assert.equal(output.trim(), 'PASS');
});

test('resumed process selection requires exact session, unique DEV ancestry and records creation identity', { skip: process.platform !== 'win32' }, () => {
  const command = `
    $ErrorActionPreference = 'Stop'
    . $env:PHASE_OWNER_MODULE
    $stamp = [DateTime]::Parse('2026-10-16T00:00:00Z').ToUniversalTime()
    $dev = [pscustomobject]@{ ProcessId=10; ParentProcessId=1; CommandLine='dev'; CreationDate=$stamp }
    $prod = [pscustomobject]@{ ProcessId=20; ParentProcessId=1; CommandLine='prod'; CreationDate=$stamp }
    $owned = [pscustomobject]@{ ProcessId=11; ParentProcessId=10; CommandLine='agent --resume "own-session"'; CreationDate=$stamp }
    $foreign = [pscustomobject]@{ ProcessId=21; ParentProcessId=20; CommandLine='agent --resume own-session'; CreationDate=$stamp }
    $prefix = [pscustomobject]@{ ProcessId=12; ParentProcessId=10; CommandLine='agent --resume own-session-foreign'; CreationDate=$stamp }
    $selected = Resolve-DevResumedProcess -Processes @($dev,$prod,$owned,$foreign,$prefix) -Session own-session -DevPid 10
    if ($selected.pid -ne 11 -or $selected.created -ne $stamp.ToString('o') -or ($selected.chain -join ',') -ne '11,10') { throw 'Wrong owned process identity' }
    foreach ($rows in @(@($dev,$prod,$foreign,$prefix), @($dev,$owned,[pscustomobject]@{ ProcessId=13; ParentProcessId=10; CommandLine='agent --resume=own-session'; CreationDate=$stamp }))) {
      $refused=$false
      try { Resolve-DevResumedProcess -Processes $rows -Session own-session -DevPid 10 } catch { $refused=$true }
      if (-not $refused) { throw 'Unsafe candidate set accepted' }
    }
    $cycle = [pscustomobject]@{ ProcessId=30; ParentProcessId=30; CommandLine='agent --resume own-session'; CreationDate=$stamp }
    $refused=$false
    try { Resolve-DevResumedProcess -Processes @($dev,$cycle) -Session own-session -DevPid 10 } catch { $refused=$true }
    if (-not $refused) { throw 'Foreign ancestry cycle accepted' }
    Write-Output 'PASS'
  `;
  const output = execFileSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', command], {
    encoding: 'utf8', windowsHide: true,
    env: { ...process.env, PHASE_OWNER_MODULE: fileURLToPath(new URL('../../evals/startup-matrix/process-owner.ps1', import.meta.url)) },
  });
  assert.equal(output.trim(), 'PASS');
});
