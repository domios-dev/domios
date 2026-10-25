import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { build } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';
import { syncBuildDirectory } from '../build-artifacts.cjs';

test('exact DEV staging keeps the next in-place frontend build byte-identical', { timeout: 30_000 }, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-frontend-provenance-'));
  try {
    fs.mkdirSync(path.join(root, 'src'));
    fs.writeFileSync(path.join(root, 'index.html'), '<script type="module" src="/entry.js"></script>');
    fs.writeFileSync(path.join(root, 'entry.js'), 'document.body.textContent = "opaque";');
    fs.writeFileSync(path.join(root, 'src/sw.js'), 'self.manifest = self.__WB_MANIFEST;');
    const buildAt = outDir => build({ root, configFile: false, logLevel: 'silent', base: './',
      plugins: [VitePWA({ strategies: 'injectManifest', srcDir: 'src', filename: 'sw.js',
        injectRegister: false, manifest: false, injectManifest: { rollupFormat: 'iife' } })],
      build: { outDir, emptyOutDir: false } });
    const snapshot = directory => Object.fromEntries(fs.readdirSync(directory, { recursive: true })
      .filter(file => fs.statSync(path.join(directory, file)).isFile()).sort()
      .map(file => [file, fs.readFileSync(path.join(directory, file), 'utf8')]));
    const source = path.join(root, 'dist'), staged = path.join(root, 'staged');
    await buildAt(source);
    fs.mkdirSync(path.join(staged, 'assets'), { recursive: true });
    const stale = path.join(staged, 'assets/old-build.js');
    fs.writeFileSync(stale, 'obsolete');
    fs.cpSync(source, staged, { recursive: true });
    const overlaid = snapshot(staged);
    await buildAt(staged);
    assert.notEqual(fs.readFileSync(path.join(staged, 'sw.js'), 'utf8'), overlaid['sw.js'],
      'the watcher adds stale assets to the precache after an overlay copy');
    syncBuildDirectory(source, staged);
    assert.equal(fs.existsSync(stale), false);
    const recorded = snapshot(staged);
    await buildAt(staged);
    assert.deepEqual(snapshot(staged), recorded);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
