import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';
import { verifyLaneMetadata } from './lane-metadata-repro.mjs';
import { laneNativeWrapper } from '../lane-native-wrapper.mjs';
import { laneBuildDirectory } from '../lane-build-dir.mjs';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

test('ordinary lanes use only an already live cache and respect explicit wrappers and opt-out', () => {
  const result = spawnSync(bash, ['-c', `set -eu
    source scripts/lane-check.sh
    RUSTC_WRAPPER=
    CODETERM_USE_SCCACHE=1
    command() { return 0; }
    ct_sccache_server_live() { return 1; }
    ct_require_prestarted_sccache() { RUSTC_WRAPPER=cache; }
    configure_lane_cache
    [ -z "$RUSTC_WRAPPER" ]
    ct_sccache_server_live() { return 0; }
    configure_lane_cache
    [ "$RUSTC_WRAPPER" = cache ]
    RUSTC_WRAPPER=custom
    configure_lane_cache
    [ "$RUSTC_WRAPPER" = custom ]
    RUSTC_WRAPPER= CODETERM_USE_SCCACHE=0
    configure_lane_cache
    [ -z "$RUSTC_WRAPPER" ]
  `], { cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /no live sccache server; this check will compile without cache/);
});

test('POSIX nested wrappers invoke the cache once immediately before rustc', () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'lane-shell-cache-'));
  try {
    const compiler = path.join(directory, 'rustc');
    const cache = path.join(directory, 'cache');
    writeFileSync(compiler, '#!/bin/sh\nprintf "%s\\n" "$@"\nexit 37\n');
    writeFileSync(cache, '#!/bin/sh\nprintf "cache\\n"\nexec "$@"\n');
    chmodSync(compiler, 0o755);
    chmodSync(cache, 0o755);
    const wrapper = path.resolve(import.meta.dirname, '../lane-rustc-wrapper.sh').replaceAll('\\', '/');
    const result = spawnSync(bash, [wrapper, wrapper, compiler.replaceAll('\\', '/'), 'opaque argument'], {
      encoding: 'utf8', env: { ...process.env, CODETERM_LANE_CACHE_WRAPPER: cache.replaceAll('\\', '/') },
    });
    assert.equal(result.status, 37, result.stderr);
    assert.equal(result.stdout, 'cache\nopaque argument\n');
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

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
      configure_lane_workspace_wrapper() { :; }
      lane_cargo_check() { [ "$CARGO_BUILD_BUILD_DIR" != canonical-build-directory ]; printf '%s' "$CARGO_BUILD_BUILD_DIR"; }
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
    CARGO_HOME="$PWD/cache-home"
    if command -v cygpath >/dev/null 2>&1; then CARGO_HOME="$(cygpath -m "$CARGO_HOME")"; fi
    export CARGO_HOME
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

test('shared resolver respects machine root, Cargo home and native home on every platform', () => {
  for (const platform of ['linux', 'darwin']) {
    assert.equal(laneBuildDirectory({ env: { CODETERM_RUST_CACHE_ROOT: '/cache', CARGO_HOME: '/cargo' }, platform }), '/cache/lanes');
    assert.equal(laneBuildDirectory({ env: { CARGO_HOME: '/cargo' }, platform }), '/cargo/codeterm-lanes');
    assert.equal(laneBuildDirectory({ env: {}, home: '/home/user', platform }), '/home/user/.cargo/codeterm-lanes');
  }
  assert.equal(laneBuildDirectory({ env: { CODETERM_RUST_CACHE_ROOT: 'C:\\cache root', CARGO_HOME: 'D:/cargo' }, platform: 'win32' }), 'C:/cache root/lanes');
  assert.equal(laneBuildDirectory({ env: { CARGO_HOME: '/d/cargo home' }, platform: 'win32' }), 'd:/cargo home/codeterm-lanes');
  assert.equal(laneBuildDirectory({ env: {}, home: 'C:\\Users\\User', platform: 'win32' }), 'C:/Users/dev/.cargo/codeterm-lanes');
});

test('lane checks cap Cargo jobs, propagate errors and request tests only when needed', () => {
  const result = spawnSync(bash, ['-c', `
    set -eu
    source scripts/lane-check.sh
    sccache() { :; }
    ct_sccache_server_live() { return 1; }
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    configure_lane_workspace_wrapper() { :; }
    enter_cargo_gateway() { return 99; }
    lane_cargo_check() { printf 'jobs=%s %s\\n' "$CARGO_BUILD_JOBS" "$*"; return 41; }
    lane_check codeterm-fixture && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-fixture --tests && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-pty codeterm-service-core --tests && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-cli --tests && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-fixture --tests --timings && exit 1 || [ "$?" = 41 ]
    lane_check codeterm-fixture --all && exit 1 || [ "$?" = 2 ]
    lane_check codeterm-fixture codeterm-cli && exit 1 || [ "$?" = 2 ]
  `], { cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8', env: { ...process.env, CODETERM_USE_SCCACHE: '1', RUSTC_WRAPPER: '' } });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /no live sccache server; this check will compile without cache/);
  assert.deepEqual(result.stdout.trim().split(/\r?\n/).filter(line => line.startsWith('jobs=')), [
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-fixture --lib',
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-fixture --lib --tests',
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-pty -p codeterm-service-core --lib --tests',
    'jobs=4 check --manifest-path cli/Cargo.toml -p codeterm-cli --bin codeterm --tests',
    'jobs=4 check --manifest-path src-tauri/Cargo.toml -p codeterm-fixture --lib --tests --timings',
  ]);
});

test('server lane routing admits only the explicit server package without real Cargo', () => {
  const result = spawnSync(bash, ['-c', `
    set -eu
    source scripts/lane-check.sh
    configure_lane_build_dir() { :; }
    configure_lane_cache() { :; }
    configure_lane_workspace_wrapper() { :; }
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    lane_cargo_check() { printf 'jobs=%s %s\\n' "$CARGO_BUILD_JOBS" "$*"; }
    lane_check domios-server --tests
    lane_check unrelated-package && exit 1 || [ "$?" = 2 ]
    lane_check domios-other && exit 1 || [ "$?" = 2 ]
  `], { cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'jobs=4 check --manifest-path src-tauri/Cargo.toml -p domios-server --lib --tests');
});


test('two worktrees use their own exported items while registry metadata stays shared', { timeout: 120_000 }, () => {
  verifyLaneMetadata();
});

test('lane workspace wrapper stays scoped to the lane subshell', () => {
  const result = spawnSync(bash, ['-c', `
    set -eu
    source scripts/lane-check.sh
    export RUSTC_WORKSPACE_WRAPPER=canonical-wrapper
    prepare_dev_frontend_dist() { :; }
    prepare_bundle_stubs() { :; }
    lane_cargo_check() { [ "$RUSTC_WORKSPACE_WRAPPER" != canonical-wrapper ]; [ -f "$RUSTC_WORKSPACE_WRAPPER" ]; }
    lane_check codeterm-fixture
    [ "$RUSTC_WORKSPACE_WRAPPER" = canonical-wrapper ]
  `], { cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
});


test('native Windows wrapper forwards a command exceeding cmd.exe limits and its exit code', { skip: process.platform !== 'win32' }, () => {
  const argument = 'unicode-λ spaces "quoted" ' + 'x'.repeat(12_000);
  const result = spawnSync(laneNativeWrapper(), [process.execPath, '-e', 'process.stdout.write(process.argv[1]); process.exit(37)', argument], { encoding: 'utf8' });
  assert.equal(result.status, 37, result.stderr);
  assert.equal(result.stdout, argument);
});

test('nested Windows wrappers pass rustc directly to the cache exactly once', { skip: process.platform !== 'win32' }, () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'lane-cache-'));
  try {
    const preload = path.join(directory, 'cache.cjs');
    writeFileSync(preload, 'process.stdout.write(JSON.stringify(process.argv.slice(1))); process.exit(37);');
    const wrapper = laneNativeWrapper();
    const args = ['C:\\compiler path\\rustc.exe', 'unicode-λ spaces "quoted" ' + 'x'.repeat(12_000)];
    const result = spawnSync(wrapper, [wrapper, ...args], {
      encoding: 'utf8',
      env: { ...process.env, CODETERM_LANE_CACHE_WRAPPER: process.execPath, NODE_OPTIONS: `--require "${preload.replaceAll('\\', '/')}"` },
    });
    assert.equal(result.status, 37, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), args);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});


test('real Windows service-core library and test targets accept the native wrapper', {
  skip: process.platform !== 'win32' || process.env.CODETERM_LANE_REAL_CRATE_TEST !== '1',
  timeout: 570_000,
}, () => {
  const result = spawnSync(bash, ['build.sh', 'lane-check', 'codeterm-service-core', '--tests'], {
    cwd: path.resolve(import.meta.dirname, '../..'), encoding: 'utf8', timeout: 540_000, maxBuffer: 16 * 1024 * 1024,
  });
  assert.equal(result.status, 0, `${result.error?.message ?? ''}\n${result.stdout}\n${result.stderr}`);
  assert.doesNotMatch(`${result.stdout}${result.stderr}`, /command line is too long/i);
});
