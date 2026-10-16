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
      [pscustomobject]@{ LocalAddress='100.84.159.37'; OwningProcess=72596 }
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
