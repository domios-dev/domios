import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '../..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const source = readFileSync(path.join(root, 'build.sh'), 'utf8').replaceAll('\r\n', '\n');
const start = source.indexOf('\n  dev-link-measure)\n');
const end = source.indexOf('\n  embedding-parity)\n', start);
const body = source.slice(start + '\n  dev-link-measure)\n'.length, end).replace(/\n    ;;\s*$/, '');

function measure(scope = 'app', status = 0, timing = false) {
  assert.ok(start > 0 && end > start);
  return spawnSync(bash, ['-c', `set -eu
    set -- dev-link-measure ${scope}
    ${timing ? 'OSTYPE=msys; CODETERM_LINK_TIMING=1; LINK=; MSYS2_ENV_CONV_EXCL=EXISTING' : ''}
    configure_dev_cargo() { CT_DEV_CARGO_PROFILE=dev; CT_DEV_CARGO_FEATURES=opaque; }
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    dev_conf_path() { printf '%s' src-tauri/tauri.dev.conf.json; }
    cargo() {
      printf '%s\\n' "$*"
      ${timing ? 'printf "%s\\n" "$LINK" "$MSYS2_ENV_CONV_EXCL" "$RUSTC_LOG"' : ''}
      return ${status}
    }
    ${body}
  `], { cwd: root, encoding: 'utf8' });
}

test('DEV timing builds only the configured application binary', () => {
  const result = measure();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'build --manifest-path src-tauri/Cargo.toml -p codeterm --bin domios-app --profile dev --no-default-features --timings');
});

for (const [scope, release] of [['cli-dev', false], ['cli-release', true]]) {
  test(`${scope} timing selects only its CLI profile`, () => {
    const result = measure(scope);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), `build --manifest-path cli/Cargo.toml --timings${release ? ' --release' : ''}`);
  });
}

test('timing preserves Cargo failure and refuses unknown scopes', () => {
  assert.equal(measure('app', 41).status, 41);
  assert.equal(measure('cli-dev', 42).status, 42);
  const result = measure('unknown');
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
});

test('Windows link timing preserves the switch and existing MSYS exclusions', () => {
  const result = measure('app', 0, true);
  assert.equal(result.status, 0, result.stderr);
  const lines = result.stdout.trim().split(/\r?\n/);
  assert.equal(lines[1], '/TIME');
  assert.equal(lines[2], 'EXISTING;LINK');
  assert.ok(lines[3].includes('rustc_codegen_ssa::back::link=info'));
});
