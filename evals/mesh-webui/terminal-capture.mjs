import { copyFile, mkdir, writeFile } from 'node:fs/promises';
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

export async function waitTerminalEvent(page, id, { type, types = [type], after = 0, deadlineMs = 30000 }) {
  if (!(deadlineMs > 0 && deadlineMs <= 120000)) throw new Error('Invalid bounded failure deadline');
  return page.evaluate(async ({ id, types, after, deadlineMs }) => {
    const probe = window.__b3TerminalProbes?.get(id);
    if (!probe) throw new Error('Unknown terminal probe');
    const matches = e => types.includes(e.type) && e.revision > after;
    const existing = probe.events.find(matches);
    if (existing) return existing;
    return new Promise((resolve, reject) => {
      const signal = AbortSignal.timeout(deadlineMs);
      const cleanup = () => {
        probe.waiters.delete(waiter);
        signal.removeEventListener('abort', expired);
      };
      const waiter = event => { if (matches(event)) { cleanup(); resolve(event); } };
      const expired = () => { cleanup(); reject(new Error(`FAIL deadline waiting for ${types.join("/")} on ${probe.tabId}`)); };
      waiter.cancel = () => { cleanup(); reject(new Error(`Terminal wait cancelled for ${probe.tabId}`)); };
      probe.waiters.add(waiter);
      signal.addEventListener('abort', expired, { once: true });
    });
  }, { id, types, after, deadlineMs });
}

function readProbeRevision(id) {
  const revision = window.__b3TerminalProbes?.get(id)?.revision;
  if (!Number.isInteger(revision)) throw new Error('Unknown terminal probe revision');
  return revision;
}

function readProbeEvents(id) {
  const events = window.__b3TerminalProbes?.get(id)?.events;
  if (!events) throw new Error('Unknown terminal probe events');
  return events;
}

export async function captureTerminal(page, { tabId, probeId, directory, provenance }) {
  const out = resolve(directory);
  const expiresAt = Date.now() + 30000;
  const signal = AbortSignal.timeout(30000);
  const within = async promise => {
    let expired;
    const failure = new Error('FAIL unstable surface: capture deadline exhausted');
    if (signal.aborted) throw failure;
    const timeout = new Promise((_, reject) => {
      expired = () => reject(failure);
      signal.addEventListener('abort', expired, { once: true });
    });
    try { return await Promise.race([promise, timeout]); }
    finally { signal.removeEventListener('abort', expired); }
  };
  await within(mkdir(out, { recursive: false }));
  const remaining = () => {
    const value = expiresAt - Date.now();
    if (value <= 0) throw new Error('FAIL unstable surface: capture deadline exhausted');
    return value;
  };
  const attempts = [];
  let stable = false;
  let failure = 'FAIL unstable surface: three capture attempts exhausted';
  for (let number = 1; number <= 3; number++) {
    remaining();
    const startRevision = await within(page.evaluate(readProbeRevision, probeId));
    const before = await within(page.evaluate(readTerminalState, { tabId }));
    const middleRevision = await within(page.evaluate(readProbeRevision, probeId));
    const candidate = await within(page.evaluate(readTerminalState, { tabId }));
    const screenshotRevision = await within(page.evaluate(readProbeRevision, probeId));
    const ready = startRevision === middleRevision && middleRevision === screenshotRevision
      && JSON.stringify(before) === JSON.stringify(candidate);
    const screenshot = ready ? `screen-attempt-${number}.png` : null;
    if (screenshot) await within(page.screenshot({ path: resolve(out, screenshot), timeout: remaining() }));
    const after = screenshot ? await within(page.evaluate(readTerminalState, { tabId })) : candidate;
    const endRevision = await within(page.evaluate(readProbeRevision, probeId));
    const unchanged = ready && screenshotRevision === endRevision
      && JSON.stringify(candidate) === JSON.stringify(after);
    const events = await within(page.evaluate(readProbeEvents, probeId));
    const attempt = { number, before, candidate, after, startRevision, middleRevision,
      screenshotRevision, endRevision, screenshot, unchanged, events };
    attempts.push(attempt);
    await writeFile(resolve(out, `attempt-${number}.json`), JSON.stringify(attempt, null, 2), { flag: 'wx' });
    if (before.truncated || candidate.truncated || after.truncated) {
      failure = 'FAIL incomplete capture: truncated terminal state';
      break;
    }
    if (unchanged) {
      stable = true;
      await copyFile(resolve(out, screenshot), resolve(out, 'screen.png'));
      break;
    }
    if (number < 3) {
      try {
        attempt.resumeEvent = await within(waitTerminalEvent(page, probeId, {
          types: ['write-parsed', 'render'], after: startRevision, deadlineMs: remaining(),
        }));
      } catch (error) {
        attempt.waitError = error.message;
        failure = `FAIL unstable surface: ${error.message}`;
      }
      await writeFile(resolve(out, `attempt-${number}-wait.json`), JSON.stringify({
        resumeEvent: attempt.resumeEvent ?? null, waitError: attempt.waitError ?? null,
      }, null, 2), { flag: 'wx' });
      if (attempt.waitError) break;
    }
  }
  const last = attempts.at(-1);
  await writeFile(resolve(out, 'state.json'), JSON.stringify({ provenance,
    before: last.before, after: last.after, unchanged: stable, revision: last.endRevision,
    events: last.events, attempts, failure: stable ? null : failure }, null, 2), { flag: 'wx' });
  await writeFile(resolve(out, 'viewport.txt'), last.after.lines
    .filter(line => line.y >= last.after.viewportY && line.y < last.after.viewportY + last.after.rows)
    .map(line => line.text).join('\n'), { flag: 'wx' });
  await writeFile(resolve(out, 'events.jsonl'), last.events.map(event => JSON.stringify(event)).join('\n') + '\n', { flag: 'wx' });
  if (!stable) throw new Error(failure);
  return { directory: out, tabId, revision: last.endRevision, state: last.after };
}

export async function releaseTerminalEvents(page, id) {
  await page.evaluate(id => {
    const probes = window.__b3TerminalProbes;
    const probe = probes?.get(id);
    if (!probe) return;
    for (const waiter of [...probe.waiters]) waiter.cancel();
    for (const handle of probe.handles) handle.dispose();
    probes.delete(id);
  }, id);
}
