import { mkdir, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { resolve } from 'node:path';

export function readTerminalState({ tabId, maxLines = 10000 }) {
  const roots = [...document.querySelectorAll('[data-testid="terminal-tab"]')]
    .filter(el => el.dataset.tabId === tabId && el.getClientRects().length);
  if (roots.length !== 1) throw new Error(`Expected one visible terminal for ${tabId}; found ${roots.length}`);
  const root = roots[0];
  const host = root.querySelector('[data-testid="terminal-xterm-host"]');
  const frame = root.querySelector('[data-testid="terminal-frame"]');
  if (frame?.dataset.terminalVisible !== 'true') throw new Error('Selected terminal is hidden in chat mode');
  const term = host?.__codetermTerminal;
  if (!term) throw new Error('DEV xterm instance unavailable; no DOM-text substitute');
  const buffer = term.buffer.active;
  const first = Math.max(0, buffer.length - maxLines);
  const lines = [];
  for (let y = first; y < buffer.length; y++) {
    const line = buffer.getLine(y);
    const cells = [];
    for (let x = 0; x < line.length; x++) {
      const cell = line.getCell(x);
      cells.push({
        x, chars: cell.getChars(), width: cell.getWidth(),
        fg: cell.getFgColor(), bg: cell.getBgColor(),
        fgMode: cell.getFgColorMode(), bgMode: cell.getBgColorMode(),
        bold: !!cell.isBold(), italic: !!cell.isItalic(), underline: !!cell.isUnderline(),
        inverse: !!cell.isInverse(), invisible: !!cell.isInvisible(),
      });
    }
    lines.push({ y, wrapped: line.isWrapped, text: line.translateToString(false), cells });
  }
  const rect = host.getBoundingClientRect();
  if (!(rect.width > 0 && rect.height > 0)) throw new Error('Terminal has no visible geometry');
  return {
    tabId, buffer: buffer.type, cols: term.cols, rows: term.rows,
    cursor: { x: buffer.cursorX, y: buffer.cursorY,
      hidden: term._core?.coreService?.isCursorHidden ?? null,
      style: term.options.cursorStyle, blink: term.options.cursorBlink },
    baseY: buffer.baseY, viewportY: buffer.viewportY, length: buffer.length,
    truncated: first !== 0, firstLine: first, lines,
    geometry: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
  };
}

export async function armTerminalEvents(page, tabId) {
  return page.evaluate(({ tabId, id }) => {
    const root = [...document.querySelectorAll('[data-testid="terminal-tab"]')]
      .find(el => el.dataset.tabId === tabId && el.getClientRects().length);
    const term = root?.querySelector('[data-testid="terminal-xterm-host"]')?.__codetermTerminal;
    if (!term) throw new Error('Cannot arm missing DEV xterm');
    const probes = window.__b3TerminalProbes ??= new Map();
    const probe = { tabId, events: [], handles: [], waiters: new Set(), revision: 0 };
    const append = (type, detail) => {
      const event = { type, detail, at: performance.now(), revision: ++probe.revision };
      probe.events.push(event);
      for (const waiter of [...probe.waiters]) waiter(event);
    };
    probe.handles.push(term.onRender(e => append('render', e)));
    probe.handles.push(term.onResize(e => append('resize', e)));
    probe.handles.push(term.onScroll(e => append('scroll', e)));
    probe.handles.push(term.onCursorMove(() => append('cursor', null)));
    probe.handles.push(term.onWriteParsed(() => append('write-parsed', null)));
    probe.handles.push(term.buffer.onBufferChange(b => append('buffer', b.type)));
    probes.set(id, probe);
    return id;
  }, { tabId, id: randomUUID() });
}

export async function waitTerminalEvent(page, id, { type, after = 0, deadlineMs = 30000 }) {
  if (!(deadlineMs > 0 && deadlineMs <= 120000)) throw new Error('Invalid bounded failure deadline');
  return page.evaluate(async ({ id, type, after, deadlineMs }) => {
    const probe = window.__b3TerminalProbes?.get(id);
    if (!probe) throw new Error('Unknown terminal probe');
    const matches = e => e.type === type && e.revision > after;
    const existing = probe.events.find(matches);
    if (existing) return existing;
    return new Promise((resolve, reject) => {
      const signal = AbortSignal.timeout(deadlineMs);
      const cleanup = () => {
        probe.waiters.delete(waiter);
        signal.removeEventListener('abort', expired);
      };
      const waiter = event => { if (matches(event)) { cleanup(); resolve(event); } };
      const expired = () => { cleanup(); reject(new Error(`FAIL deadline waiting for ${type} on ${probe.tabId}`)); };
      probe.waiters.add(waiter);
      signal.addEventListener('abort', expired, { once: true });
    });
  }, { id, type, after, deadlineMs });
}

export async function captureTerminal(page, { tabId, probeId, directory, provenance }) {
  const out = resolve(directory);
  await mkdir(out, { recursive: false });
  const before = await page.evaluate(readTerminalState, { tabId });
  const revision = await page.evaluate(id => window.__b3TerminalProbes?.get(id)?.revision ?? null, probeId);
  await page.screenshot({ path: resolve(out, 'screen.png'), timeout: 30000 });
  const after = await page.evaluate(readTerminalState, { tabId });
  const events = await page.evaluate(id => window.__b3TerminalProbes?.get(id)?.events ?? null, probeId);
  const unchanged = JSON.stringify(before) === JSON.stringify(after);
  await writeFile(resolve(out, 'state.json'), JSON.stringify({ provenance, before, after, unchanged, revision, events }, null, 2), { flag: 'wx' });
  await writeFile(resolve(out, 'viewport.txt'), after.lines
    .filter(line => line.y >= after.viewportY && line.y < after.viewportY + after.rows)
    .map(line => line.text).join('\n'), { flag: 'wx' });
  await writeFile(resolve(out, 'events.jsonl'), (events ?? []).map(event => JSON.stringify(event)).join('\n') + '\n', { flag: 'wx' });
  if (!unchanged || before.truncated || !events) throw new Error('FAIL incomplete or changing capture; original evidence retained');
  return { directory: out, tabId, revision, state: after };
}

export async function releaseTerminalEvents(page, id) {
  await page.evaluate(id => {
    const probes = window.__b3TerminalProbes;
    const probe = probes?.get(id);
    if (!probe) return;
    if (probe.waiters.size) throw new Error('Cannot release a pending event wait');
    for (const handle of probe.handles) handle.dispose();
    probes.delete(id);
  }, id);
}
