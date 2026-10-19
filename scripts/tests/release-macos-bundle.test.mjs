import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { bundleMacPair } from '../release-macos-bundle.mjs';
import { fullComponentManifest } from '../release-stage.mjs';

async function fixture(target, options, check) {
  const repo = await mkdtemp(path.join(os.tmpdir(), 'domios-mac-signing-fixture-'));
  const calls = [];
  const stage = path.join(repo, 'target', 'domios-package');
  const release = path.join(repo, 'src-tauri', 'target', ...(target.startsWith('x86_64') ? [target] : []), 'release');
  const app = path.join(release, 'bundle', 'macos', 'CodeTerm.app');
  const server = Buffer.from('fixture final signed server');
  const cli = Buffer.from('fixture final signed CLI');
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  try {
    await mkdir(path.join(stage, 'domios-server'), { recursive: true });
    await mkdir(path.join(stage, 'cli'));
    await writeFile(path.join(repo, 'package.json'), JSON.stringify({ version: '1.14.9' }));
    await writeFile(path.join(stage, 'domios-server', 'domios-server'), server);
    await writeFile(path.join(stage, 'cli', 'codeterm'), cli);
    const manifest = fullComponentManifest({ target, sourceSha: 'a'.repeat(40), serverSha256: hash(server), teamId: 'JMMWQ9LWF7' });
    await writeFile(path.join(stage, 'server-component.json'), JSON.stringify(manifest));
    const run = async (program, args) => {
      calls.push({ program, args });
      if (program === 'pnpm') {
        await mkdir(path.join(app, 'Contents', 'MacOS'), { recursive: true });
        await mkdir(path.join(app, 'Contents', 'Resources'), { recursive: true });
        await writeFile(path.join(app, 'Contents', 'MacOS', 'domios'), 'fixture native client');
        await cp(path.join(stage, 'domios-server', 'domios-server'), path.join(app, 'Contents', 'MacOS', 'domios-server'));
        await cp(path.join(stage, 'cli'), path.join(app, 'Contents', 'Resources', 'cli'), { recursive: true });
        await cp(path.join(stage, 'server-component.json'), path.join(app, 'Contents', 'Resources', 'server-component.json'));
        if (options.extra) await writeFile(path.join(app, 'Contents', 'MacOS', 'mem-bench'), 'unwanted tool');
      } else if (program === 'codesign' && args.includes('--sign') && args.at(-1) === app && options.change) {
        await writeFile(path.join(app, options.change), 'changed by outer signing');
      } else if (program === 'ditto') {
        await cp(args.at(-2), args.at(-1), { recursive: true });
      } else if (program === 'hdiutil' && args[0] === 'create') {
        await writeFile(args.at(-1), 'fixture disk image');
      } else if (program === 'hdiutil' && args[0] === 'attach') {
        const mounted = path.join(args[args.indexOf('-mountpoint') + 1], 'CodeTerm.app');
        await cp(app, mounted, { recursive: true });
        if (options.corruptImage) await writeFile(path.join(mounted, 'Contents', 'MacOS', 'domios-server'), 'corrupt image server');
      }
    };
    const invoke = () => bundleMacPair({ repo, target, run, sourceSha: 'a'.repeat(40),
      env: { APPLE_SIGNING_IDENTITY: 'Fixture Developer ID', APPLE_TEAM_ID: 'JMMWQ9LWF7', RELEASE_KEYCHAIN: 'fixture.keychain' },
      linkApplications: destination => writeFile(destination, 'fixture Applications link') });
    await check({ invoke, calls, repo, app, release, server, cli, hash });
  } finally {
    await rm(repo, { recursive: true, force: true });
  }
}

for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
  test(`${target} signs only the outer app and image, preserving the frozen pair through image readback`, async () => {
    await fixture(target, {}, async ({ invoke, calls, release, hash, server, cli }) => {
      const result = await invoke();
      assert.equal(result.serverSha256, hash(server));
      assert.equal(result.cliSha256, hash(cli));
      const tauri = calls[0];
      assert.equal(tauri.program, 'pnpm');
      assert.ok(tauri.args.includes('--no-sign'));
      assert.ok(tauri.args.includes('--no-binary-patching'));
      assert.equal(tauri.args[tauri.args.indexOf('--bundles') + 1], 'app');
      assert.deepEqual(JSON.parse(tauri.args.at(-1)), { bundle: { createUpdaterArtifacts: false } });
      assert.equal(tauri.args.includes('--target'), target.startsWith('x86_64'));
      const signing = calls.filter(call => call.program === 'codesign' && call.args.includes('--sign'));
      assert.deepEqual(signing.map(call => call.args.at(-1)), [result.app, result.dmg]);
      assert.ok(signing.every(call => !call.args.includes('--deep')));
      const requirementCheck = calls.find(call => call.program === 'codesign' && call.args.includes('--test-requirement'));
      assert.equal(requirementCheck.args[requirementCheck.args.indexOf('--test-requirement') + 1],
        '=anchor apple generic and identifier "com.codeterm.app" and certificate leaf[subject.OU] = "JMMWQ9LWF7"');
      assert.ok(calls.some(call => call.program === 'hdiutil' && call.args.includes('-readonly')));
      assert.ok(calls.some(call => call.program === 'hdiutil' && call.args[0] === 'detach'));
      assert.ok(!(await readdir(path.join(release, 'bundle'))).some(name => name.startsWith('.pair-dmg-')));
      assert.equal(await readFile(result.dmg, 'utf8'), 'fixture disk image');
    });
  });
}

for (const relative of ['Contents/MacOS/domios-server', 'Contents/Resources/cli/codeterm', 'Contents/Resources/server-component.json']) {
  test(`outer signing refuses changed frozen payload ${relative} before creating any image`, async () => {
    await fixture('aarch64-apple-darwin', { change: relative }, async ({ invoke, calls }) => {
      await assert.rejects(invoke(), /changed frozen bytes/);
      assert.ok(!calls.some(call => call.program === 'hdiutil'));
    });
  });
}

test('development executables are refused before signing the application', async () => {
  await fixture('aarch64-apple-darwin', { extra: true }, async ({ invoke, calls }) => {
    await assert.rejects(invoke(), /unexpected macOS executable inventory/);
    assert.ok(!calls.some(call => call.program === 'codesign'));
  });
});

test('corrupt disk image payload refuses packaging and detaches the owned readback mount', async () => {
  await fixture('aarch64-apple-darwin', { corruptImage: true }, async ({ invoke, calls }) => {
    await assert.rejects(invoke(), /changed frozen bytes/);
    assert.equal(calls.filter(call => call.program === 'hdiutil' && call.args[0] === 'detach').length, 1);
  });
});
