import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const checker = fileURLToPath(new URL('../check-mux-write.mjs', import.meta.url));
function check(source, file = 'crates/codeterm-service-core/src/fixture.rs') {
  const root = mkdtempSync(path.join(tmpdir(), 'mux-write-'));
  try {
    const target = path.join(root, file);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, source);
    return spawnSync(process.execPath, [checker, root], { encoding: 'utf8' });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

for (const source of [
  'let mux = svc.mux.write();',
  'let state = svc\n .mux\n .write();',
  'let guard = mux\n .write( );',
  'let _guard = state.svc.mux.write();',
]) {
  test(`rejects retained non-mut guard: ${JSON.stringify(source)}`, () => {
    const result = check(source);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /fixture.rs:1:/);
  });
}

test('accepts mutable writes, reads and mutating chains', () => {
  const result = check('let mut mux = svc.mux.write();\nlet mux = svc.mux.read();\nlet tab = svc.mux.write().create_tab();');
  assert.equal(result.status, 0, result.stderr);
});

test('rejects read-only chains across lines', () => {
  assert.equal(check('svc\n .mux\n .write()\n .get_tab(&id);').status, 1);
});

test('checks Rust outside service core', () => {
  assert.equal(check('let mux = svc.mux.write();', 'src/startup.rs').status, 1);
});

test('allows only the named contention fixture in test code', () => {
  const file = 'crates/codeterm-service-core/src/service/command_port_impl.rs';
  assert.equal(check('#[cfg(test)] mod tests { let lifecycle_owner = mux.write(); }', file).status, 0);
  assert.equal(check('let lifecycle_owner = mux.write();', file).status, 1);
  assert.equal(check('#[cfg(test)] mod tests { let other = mux.write(); }', file).status, 1);
});

test('a fixture exemption cannot hide a second occurrence', () => {
  const file = 'crates/codeterm-pty/src/dispatch/tests.rs';
  assert.equal(check('let writer = mux.write();', file).status, 0);
  assert.equal(check('let writer = mux.write();\nlet writer = mux.write();', file).status, 1);
});
