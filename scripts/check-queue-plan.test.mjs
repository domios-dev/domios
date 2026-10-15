// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { checkQueuePlan } from './check-queue-plan.mjs';

const root = path.resolve('fixture');
const names = ['codeterm', 'codeterm-service-core', 'codeterm-opaque'];
const metadata = {
  workspace_members: names,
  packages: names.map(name => ({ id: name, name, manifest_path: path.join(root, 'src-tauri', ...(name === 'codeterm' ? [] : ['crates', name]), 'Cargo.toml'), targets: [{ kind: ['lib'] }] })),
};

test('deleted and non-workspace packages cannot reach Cargo selection', () => {
  const plan = checkQueuePlan(metadata, 'app codeterm-deleted\napp codeterm-opaque', 'D\tsrc-tauri/crates/codeterm-deleted/Cargo.toml', root);
  assert.deepEqual(plan.map(pkg => pkg.name), ['codeterm-opaque']);
});

test('test paths and moved crates select tests, while core tests remain batch-only', () => {
  for (const file of ['src/tests.rs', 'src/opaque_tests.rs', 'tests/opaque.rs', 'Cargo.toml']) {
    const changes = `${file === 'Cargo.toml' ? 'R100\told/Cargo.toml\t' : 'M\t'}src-tauri/crates/codeterm-opaque/${file}\nM\tsrc-tauri/crates/codeterm-service-core/src/tests.rs`;
    const plan = checkQueuePlan(metadata, 'app codeterm-opaque\napp codeterm-service-core', changes, root);
    assert.equal(plan.find(pkg => pkg.name === 'codeterm-opaque').tests, true);
    assert.equal(plan.find(pkg => pkg.name === 'codeterm-service-core').tests, false);
  }
});

test('ordinary package changes select only their library, with root bins only on root selection', () => {
  const leaf = checkQueuePlan(metadata, 'app codeterm-opaque', 'M\tsrc-tauri/crates/codeterm-opaque/src/lib.rs', root);
  assert.deepEqual(leaf, [{ name: 'codeterm-opaque', tests: false, bins: false, lib: true }]);
  assert.equal(checkQueuePlan(metadata, 'app codeterm', '', root)[0].bins, true);
});

test('lane dispatch uses Clippy libraries, touched root bins and isolated leaf tests without all-targets', () => {
  const source = fs.readFileSync(path.resolve(import.meta.dirname, '../build.sh'), 'utf8');
  const configureProduct = source.match(/^configure_product_cargo\(\) \{[\s\S]*?^\}/m)[0];
  const start = source.indexOf('  if [ "$lane" = 1 ]; then');
  const fragment = source.slice(start, source.indexOf('  elif [ "${#pkg_args[@]}" -gt 0 ]; then', start)) + '\nfi';
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'lane-clippy-'));
  try {
    for (const rootTouched of [false, true]) {
      fs.writeFileSync(path.join(directory, 'plan'), `codeterm-service-core\t0\t0\t1\ncodeterm-opaque\t1\t0\t1\n${rootTouched ? 'codeterm\t0\t1\t1\n' : ''}`);
      const result = spawnSync(process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash', ['-c', `
        set -euo pipefail
        lane=1; plan=plan; cw="$PWD"; out=pass.log; status=0
        lane_args=(); test_packages=(); CLIPPY_ALLOW=(-Aclippy::opaque)
        CT_DEV_CARGO_FEATURES=embeddings
        ${configureProduct}
        run_check_queue_cargo() { shift; "$@"; }
        cargo() { printf '%s\\n' "$*" >> calls.log; }
        : > calls.log
        run_lane() {
        ${fragment}
        }
        run_lane
      `], { cwd: directory, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      const calls = fs.readFileSync(path.join(directory, 'calls.log'), 'utf8').trim().split('\n');
      assert.equal(calls.length, 2);
      for (const call of calls) {
        assert.match(call, /^clippy .* --lib .*--no-deps --message-format short -- -D warnings -Aclippy::opaque$/);
        assert.doesNotMatch(call, /--all-targets/);
      }
      assert.equal(calls[0].includes('--bins'), rootTouched);
      assert.ok(calls[0].includes('-p codeterm-service-core'));
      assert.ok(calls[0].includes('--no-default-features'));
      assert.ok(calls[0].includes('--features codeterm-service-core/embeddings'));
      assert.equal(calls[0].includes('--features codeterm/embeddings'), rootTouched);
      assert.doesNotMatch(calls[0], /--tests/);
      assert.match(calls[1], /-p codeterm-opaque --lib --tests/);
    }
    const cli = source.match(/^  if \[ "\$cli" = 1 \]; then[\s\S]*?^  fi/m)[0];
    assert.match(cli, /cli_targets=\(-p codeterm-cli --bins\)/);
    assert.doesNotMatch(cli, /--lib/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
