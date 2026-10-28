import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { stagedDevPaths, devResourceRoot, devServerPath } from '../dev-native-layout.mjs';
import { stageDevResources } from '../server-artifact.mjs';
import { macDevSigning, sealMacDevBundle } from '../dev-macos-bundle.mjs';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const signingEnv = { CODETERM_DEV_INSTANCE: 'opaque-instance', APPLE_SIGNING_IDENTITY: 'Developer ID Application: Fixture (ABCDE12345)', APPLE_TEAM_ID: 'ABCDE12345', RELEASE_KEYCHAIN: '/fixture.keychain' };
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');

for (const platform of ['win32', 'linux', 'darwin']) {
  for (const instance of ['', 'opaque-instance']) {
    test(`staged native resource and server paths agree with launch provenance: ${platform}/${instance || 'canonical'}`, () => {
      const layout = stagedDevPaths(path.resolve(`/lane/target/dev-instances/${instance || 'default'}/debug/codeterm${platform === 'win32' ? '.exe' : ''}`), { platform, instance });
      assert.equal(devResourceRoot(layout.client, platform), layout.resources);
      assert.equal(devServerPath(layout.client, platform), path.resolve(layout.resources, layout.serverRelative));
      assert.equal(layout.mac, platform === 'darwin' && Boolean(instance));
      if (layout.mac) assert.ok(layout.resources.endsWith(path.join('Domios.app', 'Contents', 'Resources')));
    });
  }
}

test('named macOS DEV refuses absent, ad-hoc or malformed signing configuration', () => {
  for (const env of [{}, { ...signingEnv, APPLE_SIGNING_IDENTITY: '-' }, { ...signingEnv, APPLE_TEAM_ID: 'other' }, { ...signingEnv, RELEASE_KEYCHAIN: '' }]) {
    assert.throws(() => macDevSigning(env), /requires a Developer ID/);
  }
});

async function fixture(t, run) {
  const repo = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'named-macos-dev-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const client = path.join(repo, 'target/dev-instances/opaque-instance/debug/domios-app');
  const server = path.join(path.dirname(client), 'domios-server');
  const cli = path.join(repo, 'cli/target/debug/codeterm');
  const serverSource = path.join(repo, 'src-tauri/crates/domios-server/src/main.rs');
  const cliSource = path.join(repo, 'cli/src/main.rs');
  const contract = path.join(repo, 'src-tauri/crates/codeterm-types/src/server_bootstrap.rs');
  for (const file of [client, server, cli, serverSource, cliSource, contract, ...['dist/index.html', 'skills/opaque.md', 'LICENSE', 'NOTICE', 'src-tauri/Entitlements.plist'].map(file => path.join(repo, file))]) {
    mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, `opaque-${path.basename(file)}`);
  }
  writeFileSync(contract, 'pub const API_VERSION: u32 = 3;\npub const BOOTSTRAP_PROTOCOL: u32 = 2;\n');
  const cargoLog = path.join(repo, 'server.jsonl'), cliLog = path.join(repo, 'cli.jsonl'), treeLog = path.join(repo, 'tree.txt');
  const build = 'b'.repeat(40);
  const artifact = { reason: 'compiler-artifact', target: { name: 'domios-server', kind: ['bin'], src_path: serverSource }, executable: server, profile: { test: false }, features: ['embeddings', 'mem-postgres'] };
  const finish = { reason: 'build-finished', success: true };
  writeFileSync(cargoLog, [artifact, { reason: 'build-script-executed', package_id: 'domios-server', env: [['BUILD_SHA', build], ['SERVER_CLI_SHA256', digest(cli)]] }, finish].map(JSON.stringify).join('\n'));
  writeFileSync(cliLog, [{ ...artifact, executable: cli, target: { ...artifact.target, name: 'codeterm', src_path: cliSource } }, finish].map(JSON.stringify).join('\n'));
  writeFileSync(treeLog, 'domios-server v0.0.0\ntokio v1.0\n');
  const layout = stagedDevPaths(client, { platform: 'darwin', instance: signingEnv.CODETERM_DEV_INSTANCE });
  return run({ repo, client, server, cli, layout, input: { repo, client, build, cargoLog, cliLog, treeLog, resourceRoot: layout.resources, platform: 'darwin', env: signingEnv } });
}

test('named macOS stages a signed nested pair and freezes final server bytes in inventory', async t => {
  await fixture(t, async ({ repo, client, server, cli, layout, input }) => {
    const calls = [];
    const signingRun = (program, args) => {
      assert.equal(program, 'codesign');
      calls.push(args);
      if (args.includes('--sign') && args.at(-1).endsWith('/domios-server')) writeFileSync(args.at(-1), 'opaque-signed-server');
      if (args.includes('--sign') && args.at(-1).endsWith('/Domios.app')) {
        assert.equal(readFileSync(path.resolve(layout.resources, '../MacOS/domios-server'), 'utf8'), 'opaque-signed-server');
        assert.equal(JSON.parse(readFileSync(path.join(layout.resources, 'server-component.json'))).server_sha256, digest(path.resolve(layout.resources, '../MacOS/domios-server')));
      }
    };
    const result = await stageDevResources({ ...input, signingRun });
    assert.equal(result.resource_root, layout.resources);
    assert.equal(result.manifest.server_executable, '../MacOS/domios-server');
    assert.deepEqual(result.manifest.client_identity, macDevSigning(signingEnv).clientIdentity);
    assert.equal(result.manifest.server_sha256, digest(devServerPath(layout.client, 'darwin')));
    assert.equal(digest(layout.client), digest(client));
    assert.equal(digest(path.join(layout.resources, 'cli/codeterm')), digest(cli));
    assert.equal(readFileSync(server, 'utf8'), 'opaque-domios-server');
    assert.ok(readFileSync(path.resolve(layout.resources, '../Info.plist'), 'utf8').includes('<key>CFBundleExecutable</key><string>domios-dev-app</string>'));
    assert.equal(calls.length, 4);
    assert.ok(calls.at(-1).includes('--deep'));
    assert.ok(calls.at(-1).includes('=' + result.manifest.client_identity.requirement));
    assert.ok(!calls.some(args => args.includes('--deep') && args.includes('--sign')));
    assert.ok(!calls.some(args => args.at(-1) === client || args.at(-1) === server));
    assert.throws(() => sealMacDevBundle({ root: layout.resources, repo, signing: macDevSigning(signingEnv), run: (_program, args) => {
      if (args.includes('--sign')) writeFileSync(devServerPath(layout.client, 'darwin'), 'changed');
    } }), /changed frozen pair bytes/);
  });
});

for (const boundary of ['server-sign', 'server-verify', 'app-sign', 'app-verify']) {
  test(`macOS signing refusal at ${boundary} aborts staging`, async t => {
    await fixture(t, async ({ input }) => {
      let call = 0;
      await assert.rejects(stageDevResources({ ...input, signingRun: () => {
        if (call++ === ['server-sign', 'server-verify', 'app-sign', 'app-verify'].indexOf(boundary)) throw Error('signature refused');
      } }), /signature refused/);
    });
  });
}

test('named macOS server reload refuses before inspecting or stopping the running pair', async () => {
  const { spawnSync } = await import('node:child_process');
  const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  const reload = source.match(/^reload_dev_server\(\) \{[^]*?^\}/m)[0];
  const result = spawnSync(bash, ['-c', `set -eu
${reload}
OSTYPE=darwin CODETERM_DEV_INSTANCE=opaque
validate_dev_provenance() { echo unexpected-inspect; }
node() { echo unexpected-stop; }
reload_dev_server
`], { encoding: 'utf8' });
  assert.equal(result.status, 2, result.stderr);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /sealed app pair/);
});

test('frontend output stays outside the named macOS seal and canonical paths remain unchanged', async () => {
  const { spawnSync } = await import('node:child_process');
  const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  const helpers = ['dev_resource_dir', 'dev_frontend_dir'].map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
  for (const platform of ['darwin', 'linux-gnu', 'msys']) {
    for (const instance of ['', 'opaque']) {
      const result = spawnSync(bash, ['-c', `set -eu
${helpers}
OSTYPE="$1" CODETERM_DEV_INSTANCE="$2"
tauri_debug_dir() { echo target/debug; }
pwd() { echo /fixture-checkout; }
printf '%s\\n' "$(dev_frontend_dir)"
`, '--', platform, instance], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout.trim(), platform === 'darwin' && instance ? '/fixture-checkout/dist'
        : platform === 'msys' ? 'target/debug/dist' : 'target/debug/.domios-dev-runtime/lib/codeterm/dist');
    }
  }
});
