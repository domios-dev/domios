import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const read = file => readFileSync(new URL(`../../${file}`, import.meta.url), 'utf8');

test('Windows per-user package installs the bounded helper and runs it at installer lifecycle boundaries', () => {
  const config = JSON.parse(read('src-tauri/tauri.windows.conf.json'));
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).bundle.windows.nsis.installMode, 'currentUser');
  assert.equal(config.bundle.resources['windows/account-mesh-firewall.ps1'], 'installer/account-mesh-firewall.ps1');
  const hooks = read(`src-tauri/${config.bundle.windows.nsis.installerHooks}`);
  assert.match(hooks, /!macro NSIS_HOOK_POSTINSTALL\s+!insertmacro DomiosMeshFirewall Ensure/);
  assert.match(hooks, /!macro NSIS_HOOK_PREUNINSTALL\s+\$\{If\} \$UpdateMode <> 1\s+!insertmacro DomiosMeshFirewall Remove/);
  assert.match(hooks, /-ServerProgram "\$INSTDIR\\domios-server\\domios-server\.exe"/);
  assert.match(hooks, /Pop \$0/);
  const firewallMacro = hooks.match(/!macro DomiosMeshFirewall[\s\S]*?!macroend/)[0];
  assert.doesNotMatch(firewallMacro, /\bAbort\b|\bQuit\b|RequestExecutionLevel/);
  assert.doesNotMatch(hooks, /RequestExecutionLevel/);
});

test('firewall command snapshot and lifecycle run against isolated PowerShell fakes', { skip: process.platform !== 'win32' }, () => {
  const fixture = fileURLToPath(new URL('./fixtures/account-mesh-firewall.ps1', import.meta.url));
  const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', fixture], { encoding: 'utf8', timeout: 20_000 });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  const receipt = JSON.parse(result.stdout.replace(/^\uFEFF/, ''));
  assert.deepEqual(receipt.command, {
    DisplayName: 'Domios account mesh (Tailscale only)',
    Group: 'Domios installer',
    Description: 'Installer-owned access for the Domios server from Tailscale peers.',
    Program: 'C:\\Program Files\\Domios\\domios-server\\domios-server.exe',
    Direction: 'Inbound', Action: 'Allow', Enabled: 'True', Profile: 'Any', Protocol: 'TCP',
    LocalPort: 'Any', RemotePort: 'Any', LocalAddress: 'Any',
    RemoteAddress: ['100.64.0.0/10', 'fd7a:115c:a1e0::/48'],
    EdgeTraversalPolicy: 'Block', PolicyStore: 'PersistentStore',
  });
  assert.equal(receipt.assertions, 22);
});

test('helper refuses a foreign executable before reading or changing firewall state', { skip: process.platform !== 'win32' }, () => {
  const helper = fileURLToPath(new URL('../../src-tauri/windows/account-mesh-firewall.ps1', import.meta.url));
  const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', helper, '-Operation', 'Ensure', '-ServerProgram', 'C:\\foreign.exe'], { encoding: 'utf8', timeout: 10_000 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /only manages its installed Domios server/);
});
