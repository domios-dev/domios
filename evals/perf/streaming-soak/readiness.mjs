export function ownedChatComposerMounted(document,tab) {
  const root = [...document.querySelectorAll('[data-testid="tab-session-chat"]')].find(node => node.dataset.tabId === tab);
  return Boolean(root?.querySelector('textarea[aria-label="Message composer"]'));
}
