export function readSelectedVisibleComposer({ tabId, roots = [...document.querySelectorAll('[data-testid="tab-session-chat"]')] }) {
  if (typeof tabId !== 'string' || !tabId) throw new Error('Selected tab identity is required for composer input');
  const visible = node => node.checkVisibility({ checkVisibilityCSS: true, checkOpacity: true })
    && [...node.getClientRects()].some(rect => rect.width > 0 && rect.height > 0);
  const selected = roots.filter(root => root.getAttribute('data-tab-id') === tabId && visible(root));
  if (selected.length !== 1) throw new Error(`Expected one visible chat surface for selected tab ${tabId}; found ${selected.length}`);
  const editors = [...selected[0].querySelectorAll('[data-testid="chat-composer"]')].filter(visible);
  if (editors.length !== 1) throw new Error(`Expected one visible composer for selected tab ${tabId}; found ${editors.length}`);
  if (editors[0].getAttribute('data-tab-id') !== tabId) throw new Error('Composer does not belong to selected tab');
  return { tabId };
}

export async function selectedChatSurface(page) {
  const { tabId } = await page.evaluate(readSelectedVisibleComposer, {tabId:page.__b2SelectedTabId});
  return page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]:visible`);
}
