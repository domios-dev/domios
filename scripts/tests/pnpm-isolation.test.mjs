import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const hook = require('../../.pnpmfile.cjs');
const hookPath = new URL('../../.pnpmfile.cjs', import.meta.url);

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-isolation-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const lane = path.join(root, 'lane');
  const canonical = path.join(root, 'canonical');
  fs.mkdirSync(lane);
  fs.mkdirSync(canonical);
  fs.writeFileSync(path.join(lane, 'package.json'), JSON.stringify({ name: 'fixture', version: '1.0.0', packageManager: 'pnpm@10.32.1' }));
  return { root, lane, canonical };
}

function pnpm(lane, args) {
  const corepack = path.join(path.dirname(process.execPath), 'node_modules/corepack/dist/pnpm.js');
  const command = fs.existsSync(corepack) ? process.execPath : 'pnpm';
  const prefix = fs.existsSync(corepack) ? [corepack] : [];
  const result = spawnSync(command, [...prefix, ...args], {
    cwd: lane, encoding: 'utf8', timeout: 30000, windowsHide: true,
    env: { ...process.env, CI: 'true', COREPACK_ENABLE_NETWORK: '0' },
  });
  if (result.error) throw result.error;
  return result;
}

test('copy imports override shared hardlink settings', t => {
  const { lane } = fixture(t);
  assert.equal(hook.hooks.updateConfig({ lockfileDir: lane, packageImportMethod: 'hardlink' }).packageImportMethod, 'copy');
});

test('dependency directories outside the checkout are refused', t => {
  const { lane, canonical } = fixture(t);
  for (const option of ['modulesDir', 'virtualStoreDir']) {
    assert.throws(() => hook.hooks.updateConfig({ lockfileDir: lane, [option]: canonical }), /outside this checkout/);
  }
});

for (const nested of [false, true]) {
  test(`pnpm refuses ${nested ? 'virtual store' : 'node_modules'} junction before forced offline cleanup`, t => {
    const { lane, canonical } = fixture(t);
    const modules = path.join(lane, 'node_modules');
    if (nested) fs.mkdirSync(modules);
    const link = nested ? path.join(modules, '.pnpm') : modules;
    fs.symlinkSync(canonical, link, process.platform === 'win32' ? 'junction' : 'dir');
    const marker = path.join(canonical, 'package.json');
    fs.writeFileSync(marker, 'canonical proof');
    fs.writeFileSync(path.join(canonical, '.modules.yaml'), JSON.stringify({ layoutVersion: 1 }));
    fs.copyFileSync(hookPath, path.join(lane, '.pnpmfile.cjs'));
    const result = pnpm(lane, ['install', '--offline', '--force']);
    assert.notEqual(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout + result.stderr, /pnpm refused linked dependency directory/);
    assert.equal(fs.readFileSync(marker, 'utf8'), 'canonical proof');
    assert.ok(fs.lstatSync(link).isSymbolicLink());
  });
}

test('ordinary offline install still succeeds', t => {
  const { lane } = fixture(t);
  fs.copyFileSync(hookPath, path.join(lane, '.pnpmfile.cjs'));
  const result = pnpm(lane, ['install', '--offline']);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.ok(fs.existsSync(path.join(lane, 'pnpm-lock.yaml')));
  const frozen = pnpm(lane, ['install', '--offline', '--frozen-lockfile']);
  assert.equal(frozen.status, 0, frozen.stdout + frozen.stderr);
});

test('two offline installs sharing a store have independent writable package files', t => {
  const { root, lane, canonical } = fixture(t);
  const packed = path.join(root, 'package');
  fs.mkdirSync(packed);
  fs.writeFileSync(path.join(packed, 'package.json'), JSON.stringify({ name: 'isolation-proof', version: '1.0.0' }));
  fs.writeFileSync(path.join(packed, 'proof.txt'), 'original');
  const archive = path.join(root, 'proof.tgz');
  const tar = spawnSync('tar', ['-czf', archive, '-C', root, 'package'], { encoding: 'utf8', timeout: 30000, windowsHide: true });
  assert.equal(tar.status, 0, tar.stderr || tar.error?.message);
  for (const checkout of [canonical, lane]) {
    fs.copyFileSync(hookPath, path.join(checkout, '.pnpmfile.cjs'));
    fs.writeFileSync(path.join(checkout, 'package.json'), JSON.stringify({ name: 'fixture', version: '1.0.0',
      packageManager: 'pnpm@10.32.1', dependencies: { 'isolation-proof': `file:${archive.replaceAll('\\', '/')}` } }));
    const result = pnpm(checkout, ['install', '--offline', '--store-dir', path.join(root, 'store')]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
  }
  const relative = 'node_modules/isolation-proof/proof.txt';
  assert.equal(fs.statSync(path.join(lane, relative)).nlink, 1);
  fs.writeFileSync(path.join(lane, relative), 'lane changed');
  assert.equal(fs.readFileSync(path.join(canonical, relative), 'utf8'), 'original');
});

test('unguarded pnpm purge through a junction deletes target contents', t => {
  const { lane, canonical } = fixture(t);
  fs.symlinkSync(canonical, path.join(lane, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
  fs.writeFileSync(path.join(canonical, '.modules.yaml'), JSON.stringify({ layoutVersion: 1 }));
  const marker = path.join(canonical, 'victim/package.json');
  fs.mkdirSync(path.dirname(marker));
  fs.writeFileSync(marker, 'canonical proof');
  const result = pnpm(lane, ['install', '--offline', '--force']);
  assert.match(result.stdout + result.stderr, /Recreating/);
  assert.equal(fs.existsSync(marker), false);
});
