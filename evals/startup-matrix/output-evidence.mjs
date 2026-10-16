export function snapshotOutputEvidence() {
  let totalBytes = 0;
  const text = buffer => {
    if (!buffer) return null;
    const lines = [];
    let bytes = 0;
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i)?.translateToString(true) ?? '';
      bytes += new TextEncoder().encode(line).length;
      totalBytes += new TextEncoder().encode(line).length;
      if (bytes > 1024 * 1024 || totalBytes > 8 * 1024 * 1024) throw Error('Xterm buffer exceeded evidence bound');
      lines.push(line);
    }
    return { type: buffer.type, length: buffer.length, baseY: buffer.baseY, viewportY: buffer.viewportY, text: lines.join('\n') };
  };
  return {
    at: performance.now(), visibility: document.visibilityState,
    scripts: [...document.scripts].filter(script => script.src).map(script => script.src.split(/[?#]/)[0]),
    perf: window.__outputStreamPerf ?? null,
    terminals: [...document.querySelectorAll('*')].filter(element => element.__codetermTerminal).map(element => {
      const terminal = element.__codetermTerminal, ancestors = [];
      for (let parent = element; parent; parent = parent.parentElement) {
        const style = getComputedStyle(parent);
        ancestors.push({ tag: parent.tagName, tabId: parent.getAttribute('data-tab-id'), hidden: parent.hidden,
          ariaHidden: parent.getAttribute('aria-hidden'), display: style.display, visibility: style.visibility,
          opacity: style.opacity, rect: parent.getBoundingClientRect().toJSON() });
      }
      return { tabId: element.closest('[data-tab-id]')?.getAttribute('data-tab-id') ?? null,
        cols: terminal.cols, rows: terminal.rows, active: text(terminal.buffer.active),
        normal: text(terminal.buffer.normal), alternate: text(terminal.buffer.alternate), ancestors };
    }),
  };
}

export class WireCapture {
  constructor(limit = 16 * 1024 * 1024) { this.limit = limit; this.bytes = 0; this.streams = new Map(); this.pending = new Map(); }
  arm(id) { this.pending.set(id, []); }
  begin(id, base64 = '') {
    const queued = this.pending.get(id) ?? [];
    this.pending.delete(id);
    this.add(id, base64);
    for (const bytes of queued) this.append(id, bytes);
  }
  append(id, bytes) {
    if (!this.streams.has(id)) this.streams.set(id, []);
    this.streams.get(id).push(bytes);
  }
  add(id, base64) {
    const bytes = Buffer.from(base64, 'base64');
    if (this.bytes + bytes.length > this.limit) throw Error('Raw SSE capture exceeded byte bound');
    this.bytes += bytes.length;
    if (this.pending.has(id)) this.pending.get(id).push(bytes);
    else this.append(id, bytes);
  }
  records() { return [...this.streams].map(([id, chunks]) => ({ id, bytes: Buffer.concat(chunks) })); }
}
