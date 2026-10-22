import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, mkdirSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { assertNoCompileBundleStubs, prepareCompileBundleStubs } from '../prepare-bundle-resource-stubs.mjs';

function fixture(target, action) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'codeterm-compile-stubs-'));
  try { action(root, target); } finally { rmSync(root, { recursive: true, force: true }); }
}

test('fresh Linux compile stage gets marked stubs and cannot become a package', () => {
  fixture('x86_64-unknown-linux-gnu', (root, target) => {
    assertNoCompileBundleStubs({ root });
    assert.equal(prepareCompileBundleStubs({ root, target }).created, 8);
    const stage = path.join(root, 'target/domios-package');
    assert.match(readFileSync(path.join(stage, 'LICENSE'), 'utf8'), /CODETERM_COMPILE_STUB/);
    assert.throws(() => assertNoCompileBundleStubs({ root }), /cannot be packaged/);
    assert.equal(prepareCompileBundleStubs({ root, target }).created, 0);
  });
});

test('existing real bytes are kept and partial stage is marked', () => {
  fixture('x86_64-unknown-linux-gnu', (root, target) => {
    const stage = path.join(root, 'target/domios-package');
    mkdirSync(stage, { recursive: true });
    writeFileSync(path.join(stage, 'LICENSE'), 'real license');
    assert.equal(prepareCompileBundleStubs({ root, target }).created, 7);
    assert.equal(readFileSync(path.join(stage, 'LICENSE'), 'utf8'), 'real license');
    assert.throws(() => assertNoCompileBundleStubs({ root }), /cannot be packaged/);
  });
});

test('a complete real stage stays unmarked and package eligible', () => {
  fixture('x86_64-unknown-linux-gnu', (root, target) => {
    const stage = path.join(root, 'target/domios-package');
    for (const relative of [
      'server-component.json', 'cli/codeterm', 'cli/domios', 'dist/index.html', 'skills/readme.md',
      'LICENSE', 'NOTICE', 'domios-server/domios-server',
    ]) {
      const file = path.join(stage, relative);
      mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, 'real payload');
    }
    assert.equal(prepareCompileBundleStubs({ root, target }).created, 0);
    assertNoCompileBundleStubs({ root });
  });
});

test('macOS target alias and Windows ConPTY paths are target specific', () => {
  for (const target of ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']) {
    fixture(target, root => {
      prepareCompileBundleStubs({ root, target });
      const stage = path.join(root, 'target/domios-package');
      const expected = target.endsWith('apple-darwin')
        ? `domios-server/domios-server-${target}` : 'conpty/x64/conpty.dll';
      assert.match(readFileSync(path.join(stage, expected), 'utf8'), /CODETERM_COMPILE_STUB/);
    });
  }
});
