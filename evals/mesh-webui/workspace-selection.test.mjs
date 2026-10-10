import assert from 'node:assert/strict';
import { test } from 'node:test';
import { activateAndSelectFixture, readVisibleFixtureControl } from './workspace-selection.mjs';

function fixture({ workspaceVisible = true, rowVisibleAfterActivation = true, overflow = false } = {}) {
  const clicks = [];
  const workspace = { id: '05799f42', visible: workspaceVisible && !overflow, selected: false };
  const row = { id: '6c8da59e', visible: false };
  const node = state => ({
    getAttribute: name => name === 'data-selected' ? String(state.selected) : state.id,
    checkVisibility: () => state.visible,
    getClientRects: () => [{ width: 80, height: 24 }],
  });
  const locator = state => ({
    count: async () => Number(state !== workspace || !overflow || workspace.visible),
    isVisible: async () => state.visible,
    waitFor: async () => { if (!state.visible) throw new Error('hidden'); },
    click: async options => {
      assert.equal(options, undefined, 'normal UI clicks only');
      assert.equal(state.visible, true);
      clicks.push(state.id);
      if (state === workspace) { workspace.selected = true; row.visible = rowVisibleAfterActivation; }
    },
    getByRole: () => locator(state),
  });
  const page = {
    locator: selector => locator(selector.includes('workspace-card') ? workspace : row),
    getByTestId: () => ({ count: async () => Number(overflow), isVisible: async () => workspaceVisible,
      click: async () => { clicks.push('more'); workspace.visible = workspaceVisible; } }),
    evaluate: async (fn, spec) => fn({ ...spec, nodes: [node(spec.attribute === 'data-workspace-id' ? workspace : row)] }),
  };
  const waitDom = async (_, spec) => {
    assert.equal(spec.value, 'true');
    assert.equal(workspace.selected, true);
    clicks.push('activation-confirmed');
  };
  return { page, waitDom, clicks, row };
}

test('inactive-workspace fixture row becomes visible before its normal click', async () => {
  const f = fixture();
  assert.equal(f.row.visible, false);
  await activateAndSelectFixture(f.page, { workspaceId: '05799f42', tabId: '6c8da59e' }, f.waitDom);
  assert.deepEqual(f.clicks, ['05799f42', 'activation-confirmed', '6c8da59e']);
});

test('collapsed workspace inventory expands through the visible UI control', async () => {
  const f = fixture({ overflow: true });
  await activateAndSelectFixture(f.page, { workspaceId: '05799f42', tabId: '6c8da59e' }, f.waitDom);
  assert.deepEqual(f.clicks, ['more', '05799f42', 'activation-confirmed', '6c8da59e']);
});

test('missing inventory workspace and hidden workspace or tab fail before tab click', async () => {
  for (const options of [{ workspaceVisible: false }, { rowVisibleAfterActivation: false }]) {
    const f = fixture(options);
    await assert.rejects(activateAndSelectFixture(f.page, { workspaceId: '05799f42', tabId: '6c8da59e' }, f.waitDom), /visible fixture/);
    assert.ok(!f.clicks.includes('6c8da59e'));
  }
  const f = fixture();
  await assert.rejects(activateAndSelectFixture(f.page, { tabId: '6c8da59e' }, f.waitDom), /visible fixture/);
  assert.deepEqual(f.clicks, []);
});

test('fixture control reader rejects absent, hidden and duplicate exact identities', () => {
  const node = visible => ({ getAttribute: () => 'd2d28502', checkVisibility: () => visible,
    getClientRects: () => [{ width: 80, height: 24 }] });
  for (const nodes of [[], [node(false)], [node(true), node(true)]]) {
    assert.throws(() => readVisibleFixtureControl({ selector: 'workspace-card', attribute: 'data-workspace-id', id: 'd2d28502', nodes }), /Expected one visible/);
  }
});

test('workspace lookup follows gateway or selected peer and exact fixture identity', async () => {
  const { fixtureWorkspaceId } = await import('./workspace-selection.mjs');
  const manifest = {
    mac: { peerId: 'mac-peer', tabs: [], shells: [{ tabId: '6c8da59e', workspaceId: '05799f42' }] },
    windows: { peerId: 'win-peer', tabs: [{ tabId: 'provider', workspaceId: 'd2d28502' }],
      shells: [{ tabId: 'shell', workspaceId: 'specific-workspace' }] },
  };
  assert.equal(fixtureWorkspaceId(manifest, 'mac', null, '6c8da59e'), '05799f42');
  assert.equal(fixtureWorkspaceId(manifest, 'mac', 'win-peer', 'mesh:win-peer/provider'), 'd2d28502');
  assert.equal(fixtureWorkspaceId(manifest, 'mac', 'win-peer', 'shell'), 'specific-workspace');
  assert.throws(() => fixtureWorkspaceId(manifest, 'mac', null, 'provider'), /fixture workspaceId/);
  assert.throws(() => fixtureWorkspaceId(manifest, 'mac', 'unknown-peer', 'shell'), /No fixture inventory/);
  assert.equal(fixtureWorkspaceId({ mac: { tabs: [{ tabId: 'fixture' }] } }, 'mac', null, 'fixture'), undefined);
});

test('optional workspaceId preserves selection of an already-visible fixture row', async () => {
  const f = fixture();
  f.row.visible = true;
  await activateAndSelectFixture(f.page, { tabId: '6c8da59e' }, f.waitDom);
  assert.deepEqual(f.clicks, ['6c8da59e']);
});
