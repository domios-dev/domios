import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { transformSync } from 'esbuild';

const source = readFileSync(new URL('../../src/lib/plugins/loader.ts', import.meta.url), 'utf8');
const code = transformSync(source, { loader: 'ts', format: 'cjs', define: { 'import.meta': '__viteMeta' } }).code;

function moduleVersion(plugin, registry) {
  const module = { exports: {} };
  let accept;
  const meta = {
    glob: () => ({ '/plugins/fixture/src/index.ts': { default: plugin } }),
    hot: { accept(callback) { accept = callback; } },
  };
  const require = name => {
    assert.equal(name, './registry');
    return { register: entry => registry.set(entry.meta.id, entry) };
  };
  new Function('module', 'exports', 'require', '__viteMeta', code)(module, module.exports, require, meta);
  return { exports: module.exports, accept: updated => accept(updated) };
}

test('Vite self-accept registers implementations from the replacement module', () => {
  const registry = new Map();
  const old = { meta: { id: 'fixture' }, parse: () => 'old' };
  const replacement = { meta: { id: 'fixture' }, parse: () => 'new' };
  const previousModule = moduleVersion(old, registry);
  previousModule.exports.loadBundledPlugins();
  const nextModule = moduleVersion(replacement, registry);
  previousModule.accept(nextModule.exports);
  assert.equal(registry.get('fixture').parse(), 'new');
});

test('a rejected Vite update keeps the last registered provider usable', () => {
  const registry = new Map();
  const plugin = { meta: { id: 'fixture' }, parse: () => 'working' };
  const previousModule = moduleVersion(plugin, registry);
  previousModule.exports.loadBundledPlugins();
  previousModule.accept(undefined);
  assert.equal(registry.get('fixture').parse(), 'working');
});
