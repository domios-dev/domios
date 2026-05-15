/**
 * Paste pipeline (#102) — exercises every paste path the issue flagged as
 * broken: Ctrl+V, Ctrl+Shift+V, right-click "Paste", drag plain text. Each
 * path must end with the same bracketed-paste envelope on the PTY:
 * `\x1b[200~<body>\x1b[201~`.
 *
 * Approach: write a sentinel to `navigator.clipboard`, dispatch the chord
 * (or drag event) at the focused terminal, then probe the PTY's last write
 * via `/api/panes/{id}/text` for the body. The clipboard API works inside
 * Tauri's WebView with `clipboardRead/Write` permission granted in the
 * webview features build profile (`--features webdriver`).
 *
 * Run: pnpm --filter @codeterm/webdriver-tests test:paste
 *      (script alias added below — uses the same wdio.conf.js)
 */

import { api, getFirstPane, getClaudePane } from '../helpers.js';

const SENTINEL = `paste-sentinel-${Date.now()}`;
const BRACKETED_RE = /\x1b\[200~([\s\S]*?)\x1b\[201~/;

async function focusFirstPane() {
  // Prefer the Claude pane if one exists (so we exercise the TUI path);
  // otherwise any pty-backed pane (shell) works.
  const pane = (await getClaudePane()) || (await getFirstPane());
  if (!pane) throw new Error('no pane with pty available');
  await api('POST', `/panes/${pane.id}/focus`);
  await browser.pause(500);
  // Click the terminal container so xterm.js owns focus.
  await browser.execute((paneId) => {
    const el = document.querySelector(`[data-pane-id="${paneId}"]`);
    if (el) el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
  }, pane.id);
  return pane;
}

async function writeClipboard(text) {
  return browser.execute(async (t) => {
    try {
      await navigator.clipboard.writeText(t);
      return true;
    } catch (e) {
      return String(e);
    }
  }, text);
}

async function sendKey(code, modifiers) {
  // Synth `keydown` directly on the focused terminal canvas — the most
  // reliable WebDriver path in WebView2 / WKWebView for chord modifiers.
  return browser.execute((codeStr, mods) => {
    const target = document.activeElement || document.body;
    const ev = new KeyboardEvent('keydown', {
      key: codeStr.replace(/^Key/, '').toLowerCase(),
      code: codeStr,
      ctrlKey: !!mods.ctrl,
      metaKey: !!mods.meta,
      shiftKey: !!mods.shift,
      altKey: !!mods.alt,
      bubbles: true,
      cancelable: true,
    });
    target.dispatchEvent(ev);
    return target?.tagName || null;
  }, code, modifiers);
}

async function clearScreen(paneId) {
  // Clear-screen keystroke (Ctrl+L on Unix shells) so each test sees a
  // clean tail when we probe screen text.
  await api('POST', `/panes/${paneId}/send`, { text: '\f', raw: true, enter: false });
  await browser.pause(150);
}

async function readScreen(paneId) {
  const r = await api('GET', `/panes/${paneId}/text?formatted=true`);
  return r.text || '';
}

describe('paste pipeline (#102)', () => {
  let pane;

  before(async () => {
    pane = await focusFirstPane();
    const wrote = await writeClipboard(SENTINEL);
    expect(wrote).toBe(true);
  });

  it('Ctrl+V → bracketed-paste envelope reaches PTY', async () => {
    await clearScreen(pane.id);
    await sendKey('KeyV', { ctrl: true });
    await browser.pause(400);
    const screen = await readScreen(pane.id);
    const m = screen.match(BRACKETED_RE);
    expect(m).toBeTruthy();
    expect(m[1]).toBe(SENTINEL);
  });

  it('Ctrl+Shift+V uses the same canonical pipeline', async () => {
    await clearScreen(pane.id);
    await sendKey('KeyV', { ctrl: true, shift: true });
    await browser.pause(400);
    const screen = await readScreen(pane.id);
    const m = screen.match(BRACKETED_RE);
    expect(m).toBeTruthy();
    expect(m[1]).toBe(SENTINEL);
  });

  it('right-click → Paste menu item routes through the same handler', async () => {
    await clearScreen(pane.id);
    // Open the custom ContextMenu and click its Paste item by label.
    await browser.execute((paneId) => {
      const el = document.querySelector(`[data-pane-id="${paneId}"]`);
      el?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 10, clientY: 10 }));
    }, pane.id);
    await browser.pause(200);
    const clicked = await browser.execute(() => {
      for (const b of document.querySelectorAll('button')) {
        if (b.textContent.trim().startsWith('Paste')) { b.click(); return true; }
      }
      return false;
    });
    expect(clicked).toBe(true);
    await browser.pause(500);
    const screen = await readScreen(pane.id);
    const m = screen.match(BRACKETED_RE);
    expect(m).toBeTruthy();
    expect(m[1]).toBe(SENTINEL);
  });

  it('drag text → bracketed envelope on PTY (plain-text drop path)', async () => {
    await clearScreen(pane.id);
    const dropped = await browser.execute((paneId, payload) => {
      const el = document.querySelector(`[data-pane-id="${paneId}"]`);
      if (!el) return false;
      const dt = new DataTransfer();
      dt.setData('text/plain', payload);
      const evt = new DragEvent('drop', {
        bubbles: true, cancelable: true, dataTransfer: dt,
      });
      el.dispatchEvent(evt);
      return true;
    }, pane.id, SENTINEL);
    expect(dropped).toBe(true);
    await browser.pause(400);
    const screen = await readScreen(pane.id);
    const m = screen.match(BRACKETED_RE);
    expect(m).toBeTruthy();
    expect(m[1]).toBe(SENTINEL);
  });
});
