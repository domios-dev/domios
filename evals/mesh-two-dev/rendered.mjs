export function observeRendered({ root, read, write, identity, eventId, Observer = globalThis.MutationObserver, now = Date.now }) {
  if (!root || !identity.tabId || !identity.sessionId || !identity.view || !Observer) throw new Error('Rendered view and bound identity required');
  const sample = () => {
    const id = eventId();
    if (!id) throw new Error('Rendered sample needs a correlated event id');
    write({ ...identity, eventId: id, atMs: now(), state: read(root), evidenceKind: 'rendered' });
  };
  const observer = new Observer(sample);
  observer.observe(root, { subtree: true, childList: true, attributes: true, characterData: true });
  sample();
  return () => observer.disconnect();
}
