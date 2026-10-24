import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const ensure = source.match(/^ensure_deps\(\) \{\n[\s\S]*?^\}/m)?.[0];
const fmt = source.match(/^  fmt\)\n([\s\S]*?)^    ;;/m)?.[1];
assert.ok(ensure);
assert.ok(fmt);

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'lane-dependencies-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  writeFileSync(path.join(root, 'pnpm-lock.yaml'), 'fixture');
  return root;
}

function run(root, { fail = false } = {}) {
  return spawnSync('bash', ['-c', `set -e
${ensure}
pnpm() {
  printf '%s <%s>\\n' "$CI" "$*" >> install.log
  ${fail ? 'return 42' : 'mkdir -p node_modules; touch node_modules/.modules.yaml'}
}
node() {
  test -f node_modules/.modules.yaml
  printf '%s\\n' "$*" >> format.log
}
set -- fmt --base fixture-base
${fmt}
${fmt}
`], { cwd: root, encoding: 'utf8', timeout: 30000, windowsHide: true });
}

for (const state of ['missing', 'partial', 'stale', 'ready']) {
  test(`fmt prepares ${state} dependencies once and forwards formatter arguments`, t => {
    const root = fixture(t);
    if (state !== 'missing') mkdirSync(path.join(root, 'node_modules'));
    if (state === 'stale' || state === 'ready') {
      const marker = path.join(root, 'node_modules/.modules.yaml');
      writeFileSync(marker, 'fixture');
      utimesSync(marker, state === 'stale' ? 1 : 2000000000, state === 'stale' ? 1 : 2000000000);
    }
    const result = run(root);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(existsSync(path.join(root, 'install.log')), state !== 'ready');
    if (state !== 'ready') {
      assert.equal(readFileSync(path.join(root, 'install.log'), 'utf8'), 'true <install --frozen-lockfile>\n');
      assert.match(result.stdout, /checkout-local JS dependencies from the shared pnpm store/);
    }
    assert.equal(readFileSync(path.join(root, 'format.log'), 'utf8'),
      'scripts/format-changed.mjs --base fixture-base\nscripts/format-changed.mjs --base fixture-base\n');
  });
}

test('failed frozen install preserves its exit and does not run formatting or retry unfrozen', t => {
  const root = fixture(t);
  const result = run(root, { fail: true });
  assert.equal(result.status, 42, result.stdout + result.stderr);
  assert.equal(readFileSync(path.join(root, 'install.log'), 'utf8'), 'true <install --frozen-lockfile>\n');
  assert.equal(existsSync(path.join(root, 'format.log')), false);
});
