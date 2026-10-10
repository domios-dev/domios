export function readSelectedTab({ tabId, roots = [...document.querySelectorAll('[data-testid="terminal-tab"]')] }) {
  const matches = roots.filter(root => {
    if (tabId !== undefined && root.getAttribute('data-tab-id') !== tabId) return false;
    return root.checkVisibility({ checkVisibilityCSS: true, checkOpacity: true })
      && [...root.getClientRects()].some(rect => rect.width > 0 && rect.height > 0);
  });
  if (matches.length !== 1) {
    throw new Error(`Expected one visible fixture root for ${tabId ?? 'current selection'}; found ${matches.length}`);
  }
  const root = matches[0];
  return {
    selectedTabId: root.getAttribute('data-tab-id'),
    terminalVisible: root.querySelector('[data-testid="terminal-frame"]')?.getAttribute('data-terminal-visible') ?? null,
  };
}
