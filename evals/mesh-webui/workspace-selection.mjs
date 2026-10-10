export function readVisibleFixtureControl({ selector, attribute, id, nodes = [...document.querySelectorAll(selector)] }) {
  const matches = nodes.filter(node => node.getAttribute(attribute) === id
    && node.checkVisibility({ checkVisibilityCSS: true, checkOpacity: true })
    && [...node.getClientRects()].some(rect => rect.width > 0 && rect.height > 0));
  if (matches.length !== 1) throw new Error(`Expected one visible ${selector} for ${id}; found ${matches.length}`);
  return { selected: matches[0].getAttribute('data-selected') === 'true' };
}

export async function activateAndSelectFixture(page, { workspaceId, tabId }, waitDom) {
  if (workspaceId !== undefined && (typeof workspaceId !== 'string' || !/^[a-zA-Z0-9_-]+$/.test(workspaceId))) {
    throw new Error(`Fixture ${tabId} requires a safe fixture workspaceId before selection`);
  }
  if (workspaceId !== undefined) {
    const workspaceSelector = `[data-testid="workspace-card"][data-workspace-id="${workspaceId}"]`;
    const workspace = page.locator(workspaceSelector);
    if (!await workspace.count()) {
      await waitDom(page, { selector: `${workspaceSelector}, [data-testid="workspace-card-more"]` });
      const more = page.getByTestId('workspace-card-more');
      if (await more.count() && await more.isVisible()) await more.click();
    }
    try {
      await workspace.waitFor({ state: 'visible' });
      await page.evaluate(readVisibleFixtureControl, {
        selector: '[data-testid="workspace-card"]', attribute: 'data-workspace-id', id: workspaceId,
      });
      const button = workspace.getByRole('button', { name: /^Select workspace / });
      if (!await button.isVisible()) throw new Error('workspace select button is hidden');
      await button.click();
      await waitDom(page, { selector: workspaceSelector, attr: 'data-selected', value: 'true' });
    } catch (cause) {
      throw new Error(`Cannot activate visible fixture workspace ${workspaceId} for ${tabId}`, { cause });
    }
  }
  const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`);
  try {
    await tab.waitFor({ state: 'visible' });
    await page.evaluate(readVisibleFixtureControl, {
      selector: '[data-testid="tab-item"]', attribute: 'data-tab-id', id: tabId,
    });
    await tab.click();
  } catch (cause) {
    throw new Error(`Cannot select visible fixture tab ${tabId} in workspace ${workspaceId ?? 'unspecified; populate fixture workspaceId'}`, { cause });
  }
  return tab;
}

export function fixtureWorkspaceId(manifest, gateway, peerId, tabId) {
  const machine = peerId ? Object.values(manifest).find(value => value?.peerId === peerId) : manifest[gateway];
  if (!machine) throw new Error(`No fixture inventory for gateway ${gateway}, peer ${peerId}`);
  const matches = [...machine.tabs, ...(machine.shells ?? []), ...(machine.terminals ?? [])]
    .filter(fixture => tabId === fixture.tabId || tabId === `mesh:${machine.peerId}/${fixture.tabId}`);
  const ids = [...new Set(matches.map(fixture => fixture.workspaceId))];
  if (ids.length !== 1 || (ids[0] !== undefined && (typeof ids[0] !== 'string' || !ids[0].trim()))) {
    throw new Error(`Fixture ${tabId} requires one fixture workspaceId`);
  }
  return ids[0];
}
