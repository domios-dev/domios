function domiosHitTest(x, y) {
  if (!this.isConnected) return 'stale: detached pointer target';
  const doc = this.ownerDocument;
  if (doc.defaultView !== doc.defaultView.top) return 'target_occluded: cannot verify an embedded frame target';
  if (!Number.isFinite(x) || !Number.isFinite(y)) return 'target_occluded: invalid pointer coordinates';
  let hit = doc.elementFromPoint(x, y);
  const roots = [];
  for (let node = this; node.getRootNode().host; node = node.getRootNode().host) {
    roots.push(node.getRootNode());
  }
  for (const root of roots.reverse()) {
    if (hit !== root.host) break;
    hit = root.elementFromPoint(x, y);
  }
  while (hit && hit.shadowRoot) {
    const child = hit.shadowRoot.elementFromPoint(x, y);
    if (!child || child === hit) break;
    hit = child;
  }
  for (let node = hit; node; node = node.assignedSlot || node.parentNode || node.host) {
    if (node === this) return 'clear';
  }
  if (!hit) return 'target_occluded: no element at pointer coordinates';
  const tag = hit.localName || 'element';
  const id = hit.closest('[data-sensitive], input[type="password"]') ? '' : (hit.id || '').slice(0, 80);
  return `target_occluded: covered by ${tag}${id ? '#' + id : ''}`;
}
