import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const checker = fileURLToPath(new URL('../check-file-sizes.sh', import.meta.url));

function withRepository(run) {
  const root = mkdtempSync(join(tmpdir(), 'source-size-contract-'));
  try {
    mkdirSync(join(root, 'scripts'));
    copyFileSync(checker, join(root, 'scripts/check-file-sizes.sh'));
    const initialized = spawnSync('git', ['init', '--quiet'], { cwd: root, encoding: 'utf8' });
    assert.equal(initialized.status, 0, initialized.stderr);
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function source(root, path, lines) {
  const target = join(root, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, '// fixture\n'.repeat(lines));
}

function check(root) {
  return spawnSync('bash', ['scripts/check-file-sizes.sh', '--fail'], {
    cwd: root,
    encoding: 'utf8',
  });
}

test('ignored untracked target-check downloads are generated artifacts', () => {
  withRepository((root) => {
    writeFileSync(join(root, '.gitignore'), 'target-check/\n');
    source(root, 'target-check/native/wrapper/sdk-tools.d.ts', 4172);
    const result = check(root);
    assert.equal(result.status, 0, result.stdout + result.stderr);
  });
});

test('tracked source inside an ignored target-check tree stays guarded', () => {
  withRepository((root) => {
    writeFileSync(join(root, '.gitignore'), 'target-check/\n');
    source(root, 'target-check/authored.ts', 1201);
    const tracked = spawnSync('git', ['add', '--force', 'target-check/authored.ts'], {
      cwd: root, encoding: 'utf8',
    });
    assert.equal(tracked.status, 0, tracked.stderr);
    const result = check(root);
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert.match(result.stdout, /target-check\/authored\.ts/);
  });
});

test('a target-check tree without generated-output ownership stays guarded', () => {
  withRepository((root) => {
    source(root, 'target-check/authored.py', 1201);
    const result = check(root);
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert.match(result.stdout, /target-check\/authored\.py/);
  });
});

test('ordinary source retains the exact 1200-line boundary', () => {
  withRepository((root) => {
    source(root, 'src/bounded.rs', 1200);
    const bounded = check(root);
    assert.equal(bounded.status, 0, bounded.stdout + bounded.stderr);
    source(root, 'src/oversized.rs', 1201);
    const oversized = check(root);
    assert.equal(oversized.status, 1, oversized.stdout + oversized.stderr);
    assert.match(oversized.stdout, /src\/oversized\.rs/);
  });
});
