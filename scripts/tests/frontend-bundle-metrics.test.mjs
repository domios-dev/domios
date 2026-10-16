import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { bundleMetrics, staticChunkClosure } from '../frontend-bundle-metrics.mjs';

test('startup measurement counts shared static dependencies once and excludes lazy surfaces', () => {
  const graph = new Map([
    ['main', { imports: ['react', 'chat'] }],
    ['react', { imports: [] }],
    ['chat', { imports: ['react', 'main'] }],
    ['settings', { imports: ['react'] }],
  ]);
  assert.deepEqual(staticChunkClosure(['main'], graph), ['chat', 'main', 'react']);
  assert.throws(() => staticChunkClosure(['missing'], graph), /Missing chunk/);
});

test('bundle receipt separates actual static imports from a dynamic import', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'frontend-metrics-'));
  try {
    fs.mkdirSync(path.join(root, 'assets'));
    fs.writeFileSync(path.join(root, 'index.html'), '<script type="module" src="./assets/main.js"></script>');
    const main = "import './shared.js'; export const open = () => import('./settings.js');";
    const shared = 'export const fixture = 1;';
    fs.writeFileSync(path.join(root, 'assets/main.js'), main);
    fs.writeFileSync(path.join(root, 'assets/shared.js'), shared);
    fs.writeFileSync(path.join(root, 'assets/settings.js'), 'export const settings = 2;');
    const metrics = await bundleMetrics(root);
    assert.equal(metrics.entryBytes, Buffer.byteLength(main));
    assert.equal(metrics.initialBytes, Buffer.byteLength(main + shared));
    assert.deepEqual(metrics.initialChunks, ['assets/main.js', 'assets/shared.js']);
    assert.ok(metrics.javascriptBytes > metrics.initialBytes);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
