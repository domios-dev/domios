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
  const pass = source.match(/^  configure_dev_cargo\n  local dev_status=0[\s\S]*?(?=^  grep 'cargo::core::compiler::fingerprint')/m)[0];
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
