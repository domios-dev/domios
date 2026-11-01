import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { readBuildIdentity } from './lib/build-identity.mjs';

test('build identity follows the checkout CLI binary and its own environment contract', (t) => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'cj-build-identity-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, 'cli/src/cmd'), { recursive: true });
  writeFileSync(path.join(root, 'cli/Cargo.toml'), '[package]\nname = "domios-cli"\n\n[[bin]]\nname = "helper"\npath = "src/helper.rs"\n\n[[bin]]\nname = "domios"\npath = "src/main.rs"\n');
  writeFileSync(path.join(root, 'cli/src/cmd/identify.rs'), [
    'std::env::var("DOMIOS_INSTANCE");', 'std::env::var("DOMIOS_API_PORT");',
    'std::env::var("DOMIOS_API_TOKEN");', 'std::env::var("DOMIOS_DATA_DIR");', 'std::env::var("DOMIOS_SOCKET_PATH");',
  ].join('\n'));
  writeFileSync(path.join(root, 'build.sh'), [
    'CT_INSTANCE="${DOMIOS_INSTANCE:-}"',
    'CT_PORT="${DOMIOS_API_PORT:-7686}"',
    'export DOMIOS_DEV_INSTANCE="$CT_INSTANCE"',
    'export DOMIOS_DEV_API_PORT="$CT_PORT"',
  ].join('\n'));
  const identity = readBuildIdentity(root, { env: { CARGO_TARGET_DIR: path.join(root, 'cache') } });
  assert.equal(identity.binaryName, 'domios');
  assert.equal(identity.executable, path.join(root, 'cache/debug', `domios${process.platform === 'win32' ? '.exe' : ''}`));
  assert.deepEqual(identity.request, { instance: 'DOMIOS_INSTANCE', port: 'DOMIOS_API_PORT' });
  assert.deepEqual(identity.development, { instance: 'DOMIOS_DEV_INSTANCE', port: 'DOMIOS_DEV_API_PORT' });
  assert.deepEqual(identity.cli, { instance: 'DOMIOS_INSTANCE', port: 'DOMIOS_API_PORT', apiToken: 'DOMIOS_API_TOKEN',
    dataDir: 'DOMIOS_DATA_DIR', socketPath: 'DOMIOS_SOCKET_PATH' });
  for (const instance of ['cj named', 'cj-other']) {
    const named = readBuildIdentity(root, { env: { CARGO_TARGET_DIR: path.join(root, 'ignored'), CORE_JOURNEY_REQUESTED_INSTANCE: instance } });
    assert.equal(named.executable, path.join(root, 'src-tauri/target/named-dev/debug', `domios${process.platform === 'win32' ? '.exe' : ''}`));
  }
});

test('build identity rejects incomplete environment contracts instead of guessing', (t) => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'cj-build-identity-invalid-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, 'cli/src/cmd'), { recursive: true });
  writeFileSync(path.join(root, 'cli/Cargo.toml'), '[package]\nname = "sample"\n');
  writeFileSync(path.join(root, 'cli/src/cmd/identify.rs'), '');
  writeFileSync(path.join(root, 'build.sh'), 'CT_INSTANCE="${INSTANCE:-}"');
  assert.throws(() => readBuildIdentity(root), /CT_PORT input environment/);
});
