import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

test('NSIS embeds preflight before replacement and aborts on unsafe shutdown', () => {
  const hooks = readFileSync(new URL('../../src-tauri/windows/installer-hooks.nsh', import.meta.url), 'utf8');
  const preinstall = hooks.match(/!macro NSIS_HOOK_PREINSTALL[\s\S]*?!macroend/)[0];
  assert.match(preinstall, /File .*installer-preflight\.ps1/);
  assert.match(preinstall, /File .*installer-process-model\.ps1/);
  assert.match(preinstall, /-InstallRoot "\$INSTDIR"/);
  assert.match(preinstall, /\$\{If\} \$0 != 0[\s\S]*\bAbort\b/);
});

test('installer ownership model rejects foreign paths, missing ancestry and reused PIDs', { skip: process.platform !== 'win32' }, () => {
  const fixture = fileURLToPath(new URL('./fixtures/installer-process-model.ps1', import.meta.url));
  const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', fixture], { encoding: 'utf8', timeout: 10_000 });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.deepEqual(JSON.parse(result.stdout.replace(/^\uFEFF/, '')), { assertions: 18 });
});
