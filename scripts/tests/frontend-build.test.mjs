import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { buildFrontend } from '../frontend-build.mjs';
import { makeBaseline } from '../collect-rust-warnings.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'frontend-build-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'scripts'));
  fs.mkdirSync(path.join(root, 'dist'));
  fs.writeFileSync(path.join(root, 'scripts/frontend-warnings-baseline.json'), JSON.stringify(makeBaseline([], ['frontend'])));
  return root;
}

test('successful production builds preserve unchanged assets between rebuilds', async t => {
  const root = fixture(t);
  const run = async (_command, args) => {
    if (args[0].endsWith('tsc')) return 0;
    assert.equal(args[1], 'build');
    assert.equal(args.includes('--emptyOutDir'), true);
    assert.equal(args.includes('--mode'), false);
    const directory = args[args.indexOf('--outDir') + 1];
    fs.writeFileSync(path.join(directory, 'index.html'), 'working frontend');
    fs.writeFileSync(path.join(directory, 'sw.js'), 'service worker');
    return 0;
  };
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 0);
  fs.utimesSync(path.join(root, 'dist/index.html'), 100, 100);
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 0);
  assert.equal(fs.statSync(path.join(root, 'dist/index.html')).mtimeMs, 100000);
  assert.equal(fs.readFileSync(path.join(root, 'dist/sw.js'), 'utf8'), 'service worker');
  assert.deepEqual(fs.readdirSync(path.join(root, 'node_modules/.cache')), []);
});

test('a failed Vite rebuild leaves the last working frontend intact', async t => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, 'dist/index.html'), 'working frontend');
  const run = async (_command, args) => {
    if (args[0].endsWith('tsc')) return 0;
    fs.writeFileSync(path.join(args.at(-1), 'index.html'), 'incomplete frontend');
    return 9;
  };
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 9);
  assert.equal(fs.readFileSync(path.join(root, 'dist/index.html'), 'utf8'), 'working frontend');
  assert.deepEqual(fs.readdirSync(path.join(root, 'node_modules/.cache')), []);
});

test('a new warning refuses promotion of an otherwise successful bundle', async t => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, 'dist/index.html'), 'working frontend');
  const run = async (_command, args, log) => {
    if (args[0].endsWith('tsc')) return 0;
    fs.writeFileSync(path.join(args.at(-1), 'index.html'), 'rejected frontend');
    fs.appendFileSync(log, 'Warning: new fixture warning\n');
    return 0;
  };
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 1);
  assert.equal(fs.readFileSync(path.join(root, 'dist/index.html'), 'utf8'), 'working frontend');
});

test('a warm TypeScript cache still rejects a newly introduced type error', async t => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { noEmit: true, strict: true, types: [] }, files: ['input.ts'] }));
  const input = path.join(root, 'input.ts');
  fs.writeFileSync(input, 'export const value: string = "valid";');
  let bundles = 0;
  const run = async (program, args, log) => {
    if (args[0].endsWith('tsc')) {
      const tsc = fileURLToPath(new URL('../../node_modules/typescript/bin/tsc', import.meta.url));
      const child = spawnSync(program, [tsc, ...args.slice(1)], { cwd: root, encoding: 'utf8', timeout: 30000 });
      fs.appendFileSync(log, (child.stdout ?? '') + (child.stderr ?? ''));
      if (child.error) throw child.error;
      return child.status;
    }
    fs.writeFileSync(path.join(args.at(-1), 'index.html'), `bundle ${++bundles}`);
    return 0;
  };
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 0);
  assert.ok(fs.readdirSync(path.join(root, 'node_modules/.cache')).some(name => name.endsWith('.tsbuildinfo')));
  fs.writeFileSync(input, 'export const value: string = 123;');
  assert.notEqual(await buildFrontend({ root, platform: 'fixture', run }), 0);
  assert.match(fs.readFileSync(path.join(root, 'ci-rust-warnings/frontend.log'), 'utf8'), /TS2322/);
  assert.equal(fs.readFileSync(path.join(root, 'dist/index.html'), 'utf8'), 'bundle 1');
  fs.writeFileSync(input, 'export const value: string = "fixed";');
  assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 0);
  assert.equal(bundles, 2);
});
