import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

for (const cacheRoot of [false, true]) {
  test(`shared lane directory stays scoped to lane_check, machine root=${cacheRoot}`, () => {
    const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
    const start = source.indexOf('\nsource scripts/cache-policy.sh\n');
    const end = source.indexOf('\nsource scripts/release-signing.sh\n', start);
    assert.ok(start >= 0 && end > start);
    const setup = source.slice(start, end + '\nsource scripts/release-signing.sh'.length);
    const result = spawnSync(bash, ['-c', `
      set -eu
      export CARGO_BUILD_BUILD_DIR=canonical-build-directory
      ${cacheRoot ? 'export CODETERM_RUST_CACHE_ROOT="$PWD/cache"' : 'unset CODETERM_RUST_CACHE_ROOT'}
      ${setup}
      [ "$CARGO_BUILD_BUILD_DIR" = canonical-build-directory ]
      prepare_dev_frontend_dist() { :; }
      prepare_bundle_stubs() { :; }
      cargo() { [ "$CARGO_BUILD_BUILD_DIR" != canonical-build-directory ]; printf '%s' "$CARGO_BUILD_BUILD_DIR"; }
      lane_check codeterm-fixture
      [ "$CARGO_BUILD_BUILD_DIR" = canonical-build-directory ]
    `], { cwd:path.resolve(import.meta.dirname, '../..'), encoding:'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.ok(result.stdout.replaceAll('\\', '/').endsWith(cacheRoot ? '/cache/lanes' : '/codeterm-lanes'));
  });
}

test('machine cache root selects the lane directory without a workspace hash', () => {
  const result = spawnSync(bash, ['-c', `
    set -eu
    source scripts/lane-check.sh
    unset CODETERM_RUST_CACHE_ROOT CARGO_BUILD_BUILD_DIR
    configure_lane_build_dir
    [ "$CARGO_BUILD_BUILD_DIR" = "\${CARGO_HOME:-$HOME/.cargo}/codeterm-lanes" ]
    CARGO_HOME="$PWD/cargo-home"
    configure_lane_build_dir
    [ "$CARGO_BUILD_BUILD_DIR" = "$CARGO_HOME/codeterm-lanes" ]
    CODETERM_RUST_CACHE_ROOT="$PWD/cache"
    if command -v cygpath >/dev/null 2>&1; then CODETERM_RUST_CACHE_ROOT="$(cygpath -m "$CODETERM_RUST_CACHE_ROOT")"; fi
    configure_lane_build_dir
    [ "$CARGO_BUILD_BUILD_DIR" = "$CODETERM_RUST_CACHE_ROOT/lanes" ]
    printf '%s' "$CARGO_BUILD_BUILD_DIR"
  `], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(result.stdout.replaceAll('\\', '/').endsWith('/cache/lanes'));
});

test('lane checks cap Cargo jobs, propagate errors and request tests only when needed', () => {
  const result = spawnSync(bash, ['-c', `
    set -eu
    source scripts/lane-check.sh
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    enter_cargo_gateway() { return 99; }
    cargo() { printf 'jobs=%s %s\\n' "$CARGO_BUILD_JOBS" "$*"; return 41; }
    lane_check codeterm-fixture && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-fixture --tests && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-fixture --all && exit 1 || [ "$?" = 2 ]
  `], { cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trim().split(/\r?\n/), [
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-fixture --lib',
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-fixture --lib --tests',
  ]);
});
