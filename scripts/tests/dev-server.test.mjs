import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, realpathSync, mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateReady, devServerIdentity } from '../dev-server.mjs';
const { changeKind, reloadPlan, cliSourceRoots } = createRequire(import.meta.url)('../dev-reload-inputs.cjs');
const repo = fileURLToPath(new URL('../..', import.meta.url));

test('client and build changes refuse a half reload, including deletions and previously dirty inputs', () => {
  for (const file of ['src-tauri/src/host_ports/native_client.rs', 'src-tauri/src/startup/frontend_shell.rs',
    'src-tauri/tauri.conf.json', 'src-tauri/tauri.dev.conf.json', 'src-tauri/Cargo.toml',
    'src-tauri/build.rs', 'src-tauri/crates/codeterm-client-shell/src/lib.rs', 'src-tauri/crates/codeterm-update/src/lib.rs',
    'src-tauri/crates/codeterm-types/src/server_bootstrap.rs', 'scripts/dev-build-configuration.mjs']) {
    assert.equal(changeKind(file), 'client');
    for (const [before, after] of [[{[file]: 'dirty-at-build'}, {[file]: 'edited'}], [{[file]: 'old'}, {}], [{}, {[file]: 'added'}]]) {
      assert.throws(() => reloadPlan(before, after, []), /run \.\/build.sh dev instead/);
    }
  }
  assert.throws(() => reloadPlan(null, {}, []), /run \.\/build.sh dev instead/);
});

test('server-only changes retain CLI; frontend changes stay with the watcher', () => {
  const server = 'src-tauri/crates/domios-server/src/main.rs';
  const frontend = 'src/App.tsx';
  assert.deepEqual(reloadPlan({[server]: 'old'}, {[server]: 'new'}, ['cli']), {changed: [server], cli: false});
  assert.equal(changeKind(frontend), 'frontend');
  assert.equal(reloadPlan({[frontend]: 'old'}, {[frontend]: 'new'}, ['cli']).cli, false);
  for (const file of ['build.sh', 'scripts/dev-server.mjs', 'scripts/dev-server-bootstrap.ps1',
    'scripts/dev-reload-inputs.cjs', 'scripts/server-artifact.mjs']) {
    assert.equal(changeKind(file), 'server');
    assert.equal(reloadPlan({[file]: 'old'}, {[file]: 'new'}, ['cli']).cli, false);
  }
});

test('CLI closure includes transitive local dependencies and rebuilds on their source changes', () => {
  const roots = cliSourceRoots(repo);
  assert.ok(roots.includes('cli'));
  assert.ok(roots.includes('src-tauri/crates/codeterm-plugin'));
  assert.ok(roots.includes('crates/mem-store'));
  assert.ok(!roots.includes('src-tauri/crates/domios-server'));
  for (const file of ['cli/src/main.rs', 'crates/mem-store/src/lib.rs', 'src-tauri/crates/codeterm-plugin/src/lib.rs']) {
    assert.equal(reloadPlan({[file]: 'old'}, {[file]: 'new'}, roots).cli, true);
  }
});

test('readiness binds the exact new build, process image, resources and selected instance port', () => {
  const target = { identity: {data: '/data', config: '/data/config.toml'}, executable: '/resources/domios-server', resources: '/resources', build: 'new', port: 7887 };
  const ready = { protocol: 2, api_version: 3, build: 'new', epoch: 'new-epoch', instance_id: 'instance',
    identity: target.identity, process: {pid: 42, created: '123', executable: target.executable}, resources: target.resources, api_address: '127.0.0.1:7887' };
  validateReady(ready, target);
  for (const mutation of [{build: 'old'}, {protocol: 1}, {api_address: '127.0.0.1:7685'}, {identity: {data: '/foreign', config: '/data/config.toml'}}, {identity: {data: '/data', config: '/data'}}, {epoch: ''}, {process: {...ready.process, executable: '/foreign'}}]) {
    assert.throws(() => validateReady({...ready, ...mutation}, target));
  }
});

test('bootstrap identity pins the config file, matching ResolvedInstancePaths rather than a config directory', () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'dev-server-identity-'));
  try {
    const config = path.join(directory, 'config.toml');
    writeFileSync(config, 'opaque fixture config');
    const identity = devServerIdentity(directory);
    assert.deepEqual(identity, {
      data: path.toNamespacedPath(realpathSync.native(directory)),
      config: path.toNamespacedPath(realpathSync.native(config)),
    });
    assert.notEqual(identity.config, identity.data);
    rmSync(config);
    assert.throws(() => devServerIdentity(directory), /ENOENT/);
  } finally { rmSync(directory, {recursive: true, force: true}); }
});

test('build verb never invokes whole-client cleanup, frontend build, or native compilation', () => {
  const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const body = source.match(/^reload_dev_server\(\) \{[^]*?^\}/m)[0];
  const server = source.match(/^build_dev_server\(\) \{[^]*?^\}/m)[0];
  const config = readFileSync(new URL('../firebase-build-config.sh', import.meta.url), 'utf8');
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  const directory = mkdtempSync(path.join(tmpdir(), 'dev-server-verb-'));
  mkdirSync(path.join(directory, '.domios-dev-build'));
  try {
  const result = spawnSync(bash, [], { encoding: 'utf8', input: `set -e
${source.match(/^dev_build_phase\(\) \{[^]*?^\}/m)[0]}
${body}
${server}
${config}
codeterm() {
  [ "$*" = 'mem secret get --name DOMIOS_QA_PROJECT_ID' ] || return 1
  printf opaque-qa-project
}
named_dev_isolated() { return 1; }
validate_dev_provenance() { echo server:opaque; }
tauri_debug_dir() { echo '${directory.replaceAll('\\', '/')}'; }
dev_provenance_record() { echo record; }
dev_cli_path() { echo cli; }
dev_resource_dir() { echo resources; }
dev_built_app_path() { echo app; }
enter_cargo_gateway() { echo gateway; }
configure_dev_cargo() { :; }
cli_file_sha256_or_status() { echo digest; }
node() { echo "node:$*"; }
cargo() {
  if [ "$1" = build ]; then [ "$DOMIOS_QA_PROJECT_ID" = opaque-qa-project ] || return 94; fi
  echo "cargo:$*" >&2
}
write_dev_provenance() { echo provenance; }
release_stable_build_path() { :; }
complete_build_gateway_before_handoff() { :; }
CT_DATA_DIR=data CT_PORT=7887 CT_VITE_PORT=1511 CT_DEV_CARGO_PROFILE=dev CODETERM_BUILD_SHA=sha CMD=dev-server
reload_dev_server
` });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /cargo:build .* -p domios-server --bin domios-server/, result.stdout);
  assert.doesNotMatch(result.stderr, /manifest-path cli\/Cargo.toml|-p codeterm --bin/);
  assert.match(result.stdout, /dev-server.mjs stop record data 7887/);
  assert.match(result.stdout, /dev-server.mjs start record data 7887/);
  assert.ok(result.stdout.indexOf('check-dev-server') < result.stdout.indexOf('dev-server.mjs stop'));
  assert.doesNotMatch(body, /kill_dev|start_web_watch|start_dev_static_server|build_dev_frontend/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
