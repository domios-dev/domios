// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { devBuildConfiguration, assertAppFeatureConfiguration } from './dev-build-configuration.mjs';
const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

test('product feature arguments omit Cargo default markers and enable only selected package features', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const fn = source.match(/^configure_product_cargo\(\) \{[\s\S]*?^\}/m)[0];
  const result = spawnSync(bash, ['-c', `
    set -eu
    ${fn}
    CT_DEV_CARGO_FEATURES=opaque
    configure_product_cargo -p codeterm-service-core -p codeterm-util
    printf '%s\\n' "\${CT_PRODUCT_CARGO_ARGS[*]}"
    configure_product_cargo -p codeterm
    printf '%s\\n' "\${CT_PRODUCT_CARGO_ARGS[*]}"
    configure_product_cargo -p codeterm-util
    printf '%s\\n' "\${CT_PRODUCT_CARGO_ARGS[*]}"
    configure_product_cargo --workspace
    printf '%s\\n' "\${CT_PRODUCT_CARGO_ARGS[*]}"
    CT_DEV_CARGO_FEATURES=''
    configure_product_cargo --workspace
    printf '%s\\n' "\${CT_PRODUCT_CARGO_ARGS[*]}"
  `], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trim().split(/\r?\n/), [
    '--no-default-features --features codeterm-service-core/opaque',
    '--no-default-features --features codeterm/opaque',
    '--no-default-features',
    '--no-default-features --features codeterm/opaque --features codeterm-service-core/opaque --features codeterm-mem/opaque',
    '--no-default-features',
  ]);
});

test('dev features follow manifest defaults while custom-protocol stays out of the dev cfg', () => {
  const manifest = '[features]\ndefault = ["opaque-a", "custom-protocol", "opaque-b"]\nopaque-a=[]\n';
  assert.deepEqual(devBuildConfiguration(manifest).features, ['opaque-a', 'opaque-b']);
  assert.deepEqual(devBuildConfiguration('[features]\ndefault = []').features, []);
});

test('divergent dev, root-default and core-default test features refuse another core configuration', () => {
  const manifest = features => `[features]\ndefault = [${features.map(feature => `"${feature}"`).join(',')}]\n`;
  assert.deepEqual(assertAppFeatureConfiguration(manifest(['opaque']), manifest(['opaque'])), ['opaque']);
  assert.throws(() => assertAppFeatureConfiguration(manifest(['opaque']), manifest([])), /diverged/);
  assert.throws(() => assertAppFeatureConfiguration(manifest(['opaque', 'custom-protocol']), manifest(['opaque'])), /diverged/);
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


  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('app test checks share both runner configurations and persist per-pass receipts', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const configuration = source.match(/^configure_app_test_cargo\(\) \{[\s\S]*?^\}/m)[0];
  const nextest = source.match(/^run_nextest_scope\(\) \{[\s\S]*?^\}/m)[0];
  const app = source.slice(source.indexOf('  test-one)')).match(/^      app\)[\s\S]*?^        ;;/m)[0];
  const pass = source.match(/^  local test_runner test_status[\s\S]*?^  done/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'app-test-cfg-'));
  try {
    const command = `
      set -euo pipefail
      ${configuration}
      ${nextest}
      configure_dev_cargo() { :; }
      find_app_test_archive() { return 1; }
      prepare_dev_frontend_dist() { :; }
      prepare_bundle_stubs() { :; }
      prepare_exceptional_worktree_target() { :; }
      run_check_queue_cargo() { shift; "$@"; }
      cargo() {
        printf '%s\\n' "$*" >> calls.log
        if [ "$1" = nextest ]; then return 71; fi
        return 42
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
    `;
    fs.writeFileSync(path.join(directory, 'fixture.sh'), command);
    const result = spawnSync(bash, ['fixture.sh'], { cwd: directory, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const calls = fs.readFileSync(path.join(directory, 'calls.log'), 'utf8').trim().split('\n');
    const nextestArgs = calls[0];
    const focusedArgs = calls[1];
    const checks = calls.slice(2);
    assert.equal(checks.length, 1);
    for (const [launch, check] of [[nextestArgs, checks[0]], [focusedArgs, checks[0]]]) {
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
    assert.ok(focusedArgs.includes('--no-default-features'));
    assert.equal(nextestArgs.split(' ').filter(arg => arg === '--no-default-features').length, 1);
    const markers = fs.readFileSync(path.join(directory, 'pass.log'), 'utf8').trim().split('\n');
    assert.equal(markers.length, 1);
    for (const [index, runner] of ['nextest,test-one'].entries()) {
      const args = checks[index].replace(/^check /, '').replace(/ --message-format=short$/, '');
      assert.equal(markers[index], `check-queue: app-test scope=${runner} args=${args} exit=${index ? 0 : 42}`);
      assert.ok(result.stdout.includes(markers[index]));
    }
    assert.ok(result.stdout.includes('aggregate=42'), 'the shared test pass must preserve its failure');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
