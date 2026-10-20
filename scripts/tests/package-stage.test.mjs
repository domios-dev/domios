import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const fn = name => {
  const body = source.match(new RegExp(`^${name}\\(\\) \\{\\n[\\s\\S]*?^\\}`, 'm'))?.[0];
  assert.ok(body, name);
  return body;
};
const helpers = ['package_platform', 'bundle_common_stage', 'prepare_local_macos_signing', 'notarize_local_macos_package']
  .map(fn).join('\n');
const buildEntrypoint = source.match(/^  build\)\n([\s\S]*?)^    ;;/m)?.[1];

const recorders = `
record() { printf '%s' "$1"; shift; [ "$#" -eq 0 ] || printf ' <%s>' "$@"; printf '\\n'; }
pnpm() { record pnpm "$@"; }
node() { record node "$@"; }
git() { printf 'f'%.0s {1..40}; }
prefix() { record prefix "$1"; "$@"; }
`;

function run(script, env = {}) {
  return spawnSync(bash, ['-c', `set -e\n${recorders}\n${helpers}\n${script}`], {
    encoding: 'utf8',
    env: { ...process.env, CARGO_BUILD_TARGET: '', CARGO_TARGET_DIR: '/fixture/target', CODETERM_RELEASE_FEATURES: '',
      RELEASE_KEYCHAIN: '', APPLE_SIGNING_IDENTITY: '', APPLE_TEAM_ID: '', APPLE_KEYCHAIN_PROFILE: '', ...env },
  });
}

function stage(host, env = {}) {
  const result = run(`ct_host_triple() { echo ${host}; }\nbundle_common_stage prefix`, env);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim().split('\n');
}

const sha = 'f'.repeat(40);
const stageFull = (target, client) => `node <scripts/release-stage.mjs> <stage-full> <--target> <${target}> ` +
  `<--source-sha> <${sha}> <--cargo-log> <release-artifacts/build-logs/${target}/server.jsonl> ` +
  `<--tree-log> <release-artifacts/build-logs/${target}/closure.txt> <--client-file> <${client}>`;

test('Windows stages the pair from the compiled client and bundles NSIS without recompiling', () => {
  assert.deepEqual(stage('x86_64-pc-windows-msvc'), [
    'prefix <pnpm>',
    'pnpm <tauri> <build> <--no-bundle>',
    stageFull('x86_64-pc-windows-msvc', '/fixture/target/release/domios.exe'),
    'prefix <pnpm>',
    'pnpm <tauri> <bundle> <--no-binary-patching> <--bundles> <nsis>',
  ]);
});

test('macOS compiles without the sidecar stub and assembles the signed pair app and image', () => {
  assert.deepEqual(stage('aarch64-apple-darwin'), [
    'prefix <pnpm>',
    'pnpm <tauri> <build> <--no-bundle> <--config> <{"bundle":{"externalBin":[]}}>',
    stageFull('aarch64-apple-darwin', '/fixture/target/release/domios'),
    'prefix <node>',
    'node <scripts/release-macos-bundle.mjs> <aarch64-apple-darwin>',
  ]);
});

test('Linux compiles without pair resources and bundles the release package formats', () => {
  assert.deepEqual(stage('x86_64-unknown-linux-gnu'), [
    'prefix <pnpm>',
    'pnpm <tauri> <build> <--no-bundle> <--config> <{"bundle":{"resources":[],"externalBin":[]}}>',
    stageFull('x86_64-unknown-linux-gnu', '/fixture/target/release/domios'),
    'prefix <pnpm>',
    'pnpm <tauri> <bundle> <--no-binary-patching> <--bundles> <deb,appimage>',
  ]);
});

test('an explicit cross target and release features reach every stage command', () => {
  const calls = stage('aarch64-apple-darwin', { CARGO_BUILD_TARGET: 'x86_64-apple-darwin', CODETERM_RELEASE_FEATURES: 'fixture' });
  assert.equal(calls[1], 'pnpm <tauri> <build> <--no-bundle> <--target> <x86_64-apple-darwin> <--features> <fixture> ' +
    '<--config> <{"bundle":{"externalBin":[]}}>');
  assert.equal(calls[2], stageFull('x86_64-apple-darwin', '/fixture/target/x86_64-apple-darwin/release/domios'));
  assert.equal(calls[4], 'node <scripts/release-macos-bundle.mjs> <x86_64-apple-darwin>');
});

test('an unsupported target is refused before compiling', () => {
  const result = run('ct_host_triple() { echo wasm32-unknown-unknown; }\nbundle_common_stage');
  assert.notEqual(result.status, 0);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /Unsupported package target: wasm32-unknown-unknown/);
});

test('every packaging verb uses the one shared stage helper', () => {
  for (const verb of ['build', 'release-bundle', 'install-local']) {
    const entrypoint = source.match(new RegExp(`^  ${verb}\\)\\n([\\s\\S]*?)^    ;;`, 'm'))?.[1];
    assert.match(entrypoint, /^ {4,6}bundle_common_stage\b/m, verb);
  }
  assert.doesNotMatch(source, /bundle_windows_common_stage/);
});

const security = `
security() {
  case "$1" in
    login-keychain) printf '    "/Users/fixture/Library/Keychains/login.keychain-db"\\n' ;;
    find-identity)
      [ "$5" = /Users/fixture/Library/Keychains/login.keychain-db ] || return 9
      printf '  1) AAAA "Apple Development: Fixture Person (AAAAAAAAAA)"\\n'
      printf '  2) BBBB "Developer ID Application: Fixture Person (JMMWQ9LWF7)"\\n'
      [ -z "\${SECOND_IDENTITY:-}" ] || printf '  3) CCCC "Developer ID Application: Other Person (ZZZZZZZZZZ)"\\n'
      printf '     2 valid identities found\\n' ;;
  esac
}
xcrun() { record xcrun "$@" >&3; return "\${NOTARY_STATUS:-0}"; }
`;

function signing(notarize, env = {}) {
  return run(`exec 3>&2\n${security}\nprepare_local_macos_signing ${notarize}\n` +
    'printf "keychain=%s\\nidentity=%s\\nteam=%s\\nprofile=%s\\n" "$RELEASE_KEYCHAIN" "$APPLE_SIGNING_IDENTITY" "$APPLE_TEAM_ID" "$APPLE_KEYCHAIN_PROFILE"', env);
}

test('local macOS signing resolves the login keychain Developer ID and the default notary profile', () => {
  const result = signing(1);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^keychain=\/Users\/fixture\/Library\/Keychains\/login\.keychain-db$/m);
  assert.match(result.stdout, /^identity=Developer ID Application: Fixture Person \(JMMWQ9LWF7\)$/m);
  assert.match(result.stdout, /^team=JMMWQ9LWF7$/m);
  assert.match(result.stdout, /^profile=codeterm$/m);
  assert.match(result.stderr, /xcrun <notarytool> <history> <--keychain-profile> <codeterm> <--keychain> <\/Users\/fixture\/Library\/Keychains\/login\.keychain-db>/);
});

test('local macOS signing keeps caller-supplied identity, keychain and profile', () => {
  const result = signing(1, { RELEASE_KEYCHAIN: '/fixture/own.keychain', APPLE_SIGNING_IDENTITY: 'Developer ID Application: Own (OWNTEAM123)',
    APPLE_KEYCHAIN_PROFILE: 'own-profile' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^keychain=\/fixture\/own\.keychain$/m);
  assert.match(result.stdout, /^team=OWNTEAM123$/m);
  assert.match(result.stdout, /^profile=own-profile$/m);
});

test('an ambiguous Developer ID is refused instead of guessed', () => {
  const result = signing(1, { SECOND_IDENTITY: '1' });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /expected one Developer ID Application identity .* found 2; set APPLE_SIGNING_IDENTITY/);
});

test('a missing notary profile is refused unless notarization is explicitly skipped', () => {
  const missing = signing(1, { NOTARY_STATUS: '1' });
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /keychain profile 'codeterm' is unavailable.*\n.*--no-notarize/);
  const skipped = signing(0, { NOTARY_STATUS: '1' });
  assert.equal(skipped.status, 0, skipped.stderr);
  assert.doesNotMatch(skipped.stderr, /xcrun/);
  assert.match(skipped.stdout, /skipped by --no-notarize/);
});

function build(host, args) {
  assert.ok(buildEntrypoint);
  const target = mkdtempSync(path.join(os.tmpdir(), 'domios-package-stage-'));
  try {
    const extension = { windows: 'CodeTerm_0.0.0_x64-setup.exe', macos: 'CodeTerm_0.0.0_aarch64.dmg', linux: 'CodeTerm_0.0.0_amd64.deb' };
    const platform = host.includes('windows') ? 'windows' : host.includes('darwin') ? 'macos' : 'linux';
    mkdirSync(path.join(target, 'release', 'bundle', 'pkg'), { recursive: true });
    writeFileSync(path.join(target, 'release', 'bundle', 'pkg', extension[platform]), 'fixture package');
    const fixtureTarget = target.replaceAll('\\', '/');
    const result = spawnSync(bash, ['-c', `set -e
${recorders}
${fn('package_platform')}
ct_host_triple() { echo ${host}; }
step() { record "$@"; }
node() { [ "$1" = scripts/check-release-locks.mjs ] || record node "$@"; }
require_release_source() { step source; }
prepare_local_macos_signing() { step mac-signing "$1"; }
ct_load_release_signing_key() { step updater-key; }
prepare_release_target() { export CARGO_TARGET_DIR='${fixtureTarget}'; step target; }
ensure_deps() { :; }
prepare_release_bundle_inputs() { step prepare "$1" "$CODETERM_RELEASE_COMMON_STAGE"; }
ct_gateway_dir() { echo /fixture/gateway; }
ct_snapshot_provenance() { :; }
ct_check_provenance() { :; }
bundle_common_stage() { step stage "$@"; }
notarize_local_macos_package() { step notarize "\${1#'${fixtureTarget}'}"; }
cygpath() { printf '%s' "$2"; }
set -- build ${args}
${buildEntrypoint}`], { encoding: 'utf8', env: { ...process.env, CARGO_BUILD_TARGET: '', CODETERM_RELEASE_COMMON_STAGE: '' } });
    return { ...result, lines: result.stdout.trim().split('\n').map(line => line.replace(fixtureTarget, '<target>')) };
  } finally {
    rmSync(target, { recursive: true, force: true });
  }
}

test('macOS build signs locally, stages like release CI, notarizes and names the package to install', () => {
  const result = build('aarch64-apple-darwin', '');
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.lines, [
    'source', 'mac-signing <1>', 'target', 'prepare <bundle> <1>', 'stage <ct_with_firebase_build_config>',
    'notarize </release/bundle>', 'Local package ready: <target>/release/bundle/pkg/CodeTerm_0.0.0_aarch64.dmg',
    'Install with: codeterm update install --from-file <package>',
  ]);
});

test('macOS build skips notarization only by explicit flag', () => {
  const result = build('aarch64-apple-darwin', '--no-notarize');
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.lines.slice(0, 5), ['source', 'mac-signing <0>', 'target', 'prepare <bundle> <1>', 'stage <ct_with_firebase_build_config>']);
  assert.ok(!result.lines.some(line => line.startsWith('notarize')));
});

for (const [host, name] of [['x86_64-pc-windows-msvc', 'CodeTerm_0.0.0_x64-setup.exe'], ['x86_64-unknown-linux-gnu', 'CodeTerm_0.0.0_amd64.deb']]) {
  test(`${host} build loads the updater key and stages like release CI`, () => {
    const result = build(host, '');
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.lines, [
      'source', 'updater-key', 'target', 'prepare <bundle> <1>', 'stage <ct_with_firebase_build_config>',
      `Local package ready: <target>/release/bundle/pkg/${name}`, 'Install with: codeterm update install --from-file <package>',
    ]);
  });
}

test('build refuses unknown options before touching source or signing', () => {
  const result = build('aarch64-apple-darwin', '--sign-later');
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /Usage: \.\/build\.sh build \[--no-notarize\]/);
});
