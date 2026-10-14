// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { devBuildConfiguration } from './dev-build-configuration.mjs';
const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

test('dev features follow manifest defaults while custom-protocol stays out of the dev cfg', () => {
  const manifest = '[features]\ndefault = ["opaque-a", "custom-protocol", "opaque-b"]\nopaque-a=[]\n';
  assert.deepEqual(devBuildConfiguration(manifest).features, ['opaque-a', 'opaque-b']);
  assert.deepEqual(devBuildConfiguration('[features]\ndefault = []').features, []);
});

test('dev launch and the regular library check share profile, features, flags and no test cfg', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const fn = source.match(/^configure_dev_cargo\(\) \{[\s\S]*?^\}/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-cargo-config-'));
  try {
    const result = spawnSync(bash, ['-c', `
      ${fn}
      CT_GATEWAY_SCRIPT_ROOT="$PWD"
      RUSTFLAGS='--cfg opaque_dev'
      CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false
      export RUSTFLAGS CARGO_PROFILE_DEV_DEBUG_ASSERTIONS
      configure_dev_cargo
      printf '%s\\n' "\${CT_DEV_CHECK_ARGS[*]}" "\${CT_DEV_TAURI_ARGS[*]}" "$RUSTFLAGS" "$CARGO_PROFILE_DEV_DEBUG_ASSERTIONS"
    `], { cwd: root, env: { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH }, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const [check, launch, flags, debug] = result.stdout.trim().split(/\r?\n/);
    const features = devBuildConfiguration(fs.readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8')).features.join(',');
    assert.match(check, /-p codeterm --lib --profile dev --no-default-features/);
    assert.match(launch, /-- --profile dev$/);
    if (features) {
      assert.ok(check.includes(`--features ${features}`));
      assert.ok(launch.includes(`--features ${features}`));
    }
    assert.doesNotMatch(check, /--all-targets|--tests|--release/);
    assert.equal(flags, '--cfg opaque_dev');
    assert.equal(debug, 'false');
    for (const line of source.split('\n').filter(line => line.includes('pnpm tauri dev') && !line.includes('screen -dmS'))) {
      assert.ok(line.includes('CT_DEV_TAURI_ARGS'), line);
    }
    assert.match(source, /cargo check "\$\{CT_DEV_CHECK_ARGS\[@\]\}" --message-format=short >> "\$out" 2>&1 \|\| dev_status=\$\?/);
    assert.match(source, /CODETERM_DEV=1 CODETERM_WEB_DIST="\$cw\/dist" CARGO_TARGET_DIR="\$cw\/target\/app"/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('check-queue reports dev library arguments and its own exit in the summary and full log', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const pass = source.match(/^  configure_dev_cargo\n  local dev_status=0[\s\S]*?(?=^  local test_runner test_status)/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-cargo-receipt-'));
  try {
    for (const exit of [0, 42]) {
      const result = spawnSync(bash, ['-c', `
        set -euo pipefail
        configure_dev_cargo() { CT_DEV_CHECK_ARGS=(--lib --profile opaque-profile --features opaque-feature); }
        cargo() { return ${exit}; }
        run_pass() {
          cw="$PWD"
          out=pass.log
          : > "$out"
          status=17
          ${pass}
          printf 'aggregate=%s\\n' "$status"
        }
        run_pass
      `], { cwd: directory, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      const marker = `check-queue: dev-lib args=--lib --profile opaque-profile --features opaque-feature exit=${exit}`;
      assert.equal(result.stdout.trim(), `${marker}\naggregate=${exit || 17}`);
      assert.equal(fs.readFileSync(path.join(directory, 'pass.log'), 'utf8').trim(), marker);
    }
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('app test checks share both runner configurations and persist per-pass receipts', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const configuration = source.match(/^configure_app_test_cargo\(\) \{[\s\S]*?^\}/m)[0];
  const nextest = source.match(/^run_nextest_scope\(\) \{[\s\S]*?^\}/m)[0];
  const app = source.slice(source.indexOf('  test-one)')).match(/^      app\)[\s\S]*?^        ;;/m)[0];
  const pass = source.match(/^  local test_runner test_status[\s\S]*?(?=^  grep 'cargo::core::compiler::fingerprint')/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'app-test-cfg-'));
  try {
    const result = spawnSync(bash, ['-c', `
      set -euo pipefail
      ${configuration}
      ${nextest}
      prepare_dev_frontend_dist() { :; }
      prepare_bundle_stubs() { :; }
      prepare_exceptional_worktree_target() { :; }
      cargo() {
        printf '%s\\n' "$*" >> calls.log
        if [ "$1" = nextest ]; then return 71; fi
        case "$*" in *--no-default-features*) return 42 ;; esac
      }
      scripts/test-capped.sh() { printf 'test-one %s\\n' "$*" >> calls.log; }
      run_nextest_scope app || [ "$?" = 71 ]
      test_filter=opaque-filter
      case app in
      ${app}
      esac
      run_checks() {
        cw="$PWD"
        out=pass.log
        status=0
        ${pass}
        printf 'aggregate=%s\\n' "$status"
      }
      run_checks
    `], { cwd: directory, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const calls = fs.readFileSync(path.join(directory, 'calls.log'), 'utf8').trim().split('\n');
    const nextestArgs = calls[0];
    const focusedArgs = calls[1];
    const checks = calls.slice(2);
    assert.equal(checks.length, 2);
    for (const [launch, check] of [[nextestArgs, checks[0]], [focusedArgs, checks[1]]]) {
      for (const option of ['--manifest-path', '-p']) {
        const value = launch.split(' ')[launch.split(' ').indexOf(option) + 1];
        assert.ok(check.includes(`${option} ${value}`), `${launch} vs ${check}`);
      }
      assert.ok(launch.includes('--lib') && check.includes('--lib'));
      assert.equal(launch.includes('--no-default-features'), check.includes('--no-default-features'));
      assert.equal(launch.includes('--all-features'), check.includes('--all-features'));
      const featureValue = args => args.match(/(?:--features|-F) (\S+)/)?.[1];
      assert.equal(featureValue(launch), featureValue(check));
      assert.ok(check.includes('--tests'), 'test cfg must be checked');
      assert.ok(check.includes('--profile test'));
      assert.doesNotMatch(launch, /--release|--profile|--cargo-profile/);
    }
    assert.ok(nextestArgs.includes('--no-default-features'));
    assert.ok(!focusedArgs.includes('--no-default-features'));
    const markers = fs.readFileSync(path.join(directory, 'pass.log'), 'utf8').trim().split('\n');
    assert.equal(markers.length, 2);
    for (const [index, runner] of ['nextest', 'test-one'].entries()) {
      const args = checks[index].replace(/^check /, '').replace(/ --message-format=short$/, '');
      assert.equal(markers[index], `check-queue: app-test scope=${runner} args=${args} exit=${index ? 0 : 42}`);
      assert.ok(result.stdout.includes(markers[index]));
    }
    assert.ok(result.stdout.includes('aggregate=42'), 'a later successful pass must preserve the failed pass');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
