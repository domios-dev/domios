import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const helper = fileURLToPath(new URL('../windows-dev-listeners.ps1', import.meta.url));

test('Windows listener discovery covers blank-state dual-stack endpoints and netstat fallback without port-only identity',
  { skip: process.platform !== 'win32', timeout: 15_000 }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-listener-cases-'));
    const script = path.join(directory, 'cases.ps1');
    fs.writeFileSync(script, `param([string] $Helper)
. $Helper
$connections = @(
  @{ LocalAddress='127.0.0.1'; LocalPort=7717; RemoteAddress='0.0.0.0'; RemotePort=0; State=$null; OwningProcess=42 },
  @{ LocalAddress='::1'; LocalPort=7717; RemoteAddress='::'; RemotePort=0; State=0; OwningProcess=42 },
  @{ LocalAddress='::'; LocalPort=7718; RemoteAddress='::'; RemotePort=0; State='Listen'; OwningProcess=43 },
  @{ LocalAddress='127.0.0.1'; LocalPort=7719; RemoteAddress='127.0.0.1'; RemotePort=1234; State='Established'; OwningProcess=44 },
  @{ LocalAddress='127.0.0.1'; LocalPort=7720; RemoteAddress='127.0.0.1'; RemotePort=1234; State=$null; OwningProcess=45 },
  @{ LocalAddress='127.0.0.1'; LocalPort=7721; RemoteAddress='0.0.0.0'; RemotePort=0; State='Bound'; OwningProcess=46 },
  @{ LocalAddress='127.0.0.1'; LocalPort=7685; RemoteAddress='0.0.0.0'; RemotePort=0; State=$null; OwningProcess=47 }
)
$netstat = @(
  ' TCP [::]:7718 [::]:0 LISTENING 43',
  ' TCP [fd7a:115c:a1e0::1]:7722 [::]:0 LISTENING 48',
  ' TCP 127.0.0.1:7723 0.0.0.0:0 LISTENING 49',
  ' TCP 127.0.0.1:7724 127.0.0.1:7717 ESTABLISHED 50',
  ' TCP 127.0.0.1:17717 0.0.0.0:0 LISTENING 51'
)
$rows = @(ConvertTo-DevTcpListeners $connections $netstat)
$keys = @($rows | ForEach-Object { "$($_.port):$($_.pid)" }) -join ','
if ($keys -ne '7685:47,7717:42,7718:43,7722:48,7723:49,17717:51') { throw "Wrong endpoint selection: $keys" }
$self = [Diagnostics.Process]::GetCurrentProcess()
try {
  $image = $self.MainModule.FileName
  $script:selected = @([pscustomobject]@{ port=7717; pid=$self.Id })
  function Get-DevTcpListeners { return $script:selected }
  if ((Get-DevListenerPid 7717 $image) -ne $self.Id) { throw 'Exact live image not selected' }
  foreach ($case in @(@{ port=7685; image=$image }, @{ port=7717; image="$image-foreign" }, @{ port=17717; image=$image })) {
    $refused = $false
    try { Get-DevListenerPid $case.port $case.image | Out-Null } catch { $refused = $true }
    if (-not $refused) { throw 'Unsafe listener accepted' }
  }
  $script:selected += [pscustomobject]@{ port=7717; pid=999 }
  $refused = $false
  try { Get-DevListenerPid 7717 $image | Out-Null } catch { $refused = $true }
  if (-not $refused) { throw 'Ambiguous ownership accepted' }
} finally { $self.Dispose() }
Write-Output 'listener cases passed'
`);
    try {
      const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-Helper', helper],
        { encoding: 'utf8', windowsHide: true, timeout: 12_000 });
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /listener cases passed/);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
