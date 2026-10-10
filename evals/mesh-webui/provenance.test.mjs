import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readSelectedTab } from './provenance.mjs';

function root(tabId, { visible = true, width = 80, height = 24, terminalVisible = 'true' } = {}) {
  return {
    getAttribute: name => name === 'data-tab-id' ? tabId : null,
    checkVisibility: () => visible,
    getClientRects: () => [{ width, height }],
    querySelector: () => ({ getAttribute: () => terminalVisible }),
  };
}

test('requested fixture wins over the first mounted General Agent root with geometry', () => {
  const roots = [root('46137477', { visible: false }), root('fixture')];
  assert.deepEqual(readSelectedTab({ roots, tabId: 'fixture' }), {
    selectedTabId: 'fixture', terminalVisible: 'true',
  });
});

test('exact requested mesh identity wins even when another root is visible', () => {
  const roots = [root('fixture'), root('mesh:peer/fixture', { terminalVisible: 'false' })];
  assert.deepEqual(readSelectedTab({ roots, tabId: 'mesh:peer/fixture' }), {
    selectedTabId: 'mesh:peer/fixture', terminalVisible: 'false',
  });
});

test('hidden, absent, zero-size and duplicate requested roots fail loudly', () => {
  for (const roots of [
    [root('other')],
    [root('other'), root('fixture', { visible: false })],
    [root('fixture', { width: 0 })],
    [root('fixture', { height: 0 })],
    [root('fixture'), root('fixture')],
  ]) {
    assert.throws(() => readSelectedTab({ roots, tabId: 'fixture' }), /Expected one visible fixture root for fixture/);
  }
});

test('gateway return without a requested tab requires a unique visible root', () => {
  assert.equal(readSelectedTab({ roots: [root('hidden', { visible: false }), root('selected')] }).selectedTabId, 'selected');
  assert.throws(() => readSelectedTab({ roots: [root('one'), root('two')] }), /found 2/);
  assert.throws(() => readSelectedTab({ roots: [] }), /found 0/);
});
