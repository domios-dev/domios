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

// Short and unique-enough — long timestamps get truncated when Claude
// renders the paste inside its input field, which breaks substring match.
// 6 random chars in base32 keeps it under one terminal cell row.
const SENTINEL = `s${Math.random().toString(36).slice(2, 8)}xq`;

async function focusVisiblePane() {
  // Wait for any pane with an xterm helper textarea to render.
  await browser.waitUntil(
    async () => browser.execute(() => document.querySelector('.xterm-helper-textarea') !== null),
    { timeout: 10000, timeoutMsg: 'no rendered xterm pane in DOM' },
  );
  const result = await browser.execute(() => {
    const root = document.querySelector('[data-pane-id]');
    const id = root?.getAttribute('data-pane-id') || null;
    const ta = document.querySelector('.xterm-helper-textarea');
    ta?.focus();
    return { ok: document.activeElement === ta, paneId: id };
  });
  if (!result.ok || !result.paneId) {
    throw new Error(`could not focus xterm: ${JSON.stringify(result)}`);
  }
  return result.paneId;
}

async function refocusXterm() {
  await browser.execute(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    ta?.focus();
  });
}

// Wait for the sentinel (or a fresh "[Pasted text #N]" placeholder above
// the per-pane baseline) to appear on the SPECIFIC pane the action targeted.
// "Any pane anywhere" is too loose — a placeholder bump in a sibling
// Claude pane could satisfy the assertion even if our path never fired.
async function waitForSentinelOnPane(paneId, baselineForPane, timeoutMs = 5000) {
  const placeholderRe = /\[Pasted text #(\d+)/g;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const out = await api('GET', `/panes/${paneId}/output?offset=0&strip_ansi=true`);
    const text = out.output || '';
    if (text.includes(SENTINEL)) return { kind: 'sentinel' };
    const placeholders = [...text.matchAll(placeholderRe)].map((m) => Number(m[1]));
    const fresh = placeholders.filter((n) => n > baselineForPane);
    if (fresh.length > 0) return { kind: 'placeholder', n: Math.max(...fresh) };
    await browser.pause(200);
  }
  return null;
}

async function focusedPaneBaseline(paneId) {
  const out = await api('GET', `/panes/${paneId}/output?offset=0&strip_ansi=true`);
  const text = out.output || '';
  const ns = [...text.matchAll(/\[Pasted text #(\d+)/g)].map((m) => Number(m[1]));
  return ns.length ? Math.max(...ns) : -1;
}


async function writeClipboard(text) {
  // executeAsync resolves the Promise on the page; plain `execute` returns
  // the Promise object verbatim (serialized as `{}`) which we can't assert on.
  return browser.executeAsync((t, done) => {
    navigator.clipboard
      .writeText(t)
      .then(() => done(true))
      .catch((e) => done(String(e)));
  }, text);
}

// xterm.js reads from its own hidden textarea — synthetic KeyboardEvents
// don't reach attachCustomKeyEventHandler. browser.keys() goes through the
// real WebDriver Actions API and hits xterm's input target as if you typed.
async function sendChord(keys) {
  await browser.keys(keys);
}

describe('paste pipeline (#102)', () => {
  let focusedPaneId;

  before(async () => {
    // Force a hard reload so the frontend re-fetches pane state from the
    // backend instead of rendering a stale React snapshot.
    await browser.refresh();
    await browser.pause(2000);
    focusedPaneId = await focusVisiblePane();
    const wrote = await writeClipboard(SENTINEL);
    expect(wrote).toBe(true);
  });

  it('Ctrl+V → sentinel lands in the focused PTY', async () => {
    const baseline = await focusedPaneBaseline(focusedPaneId);
    await refocusXterm();
    await sendChord(['Control', 'v']);
    const hit = await waitForSentinelOnPane(focusedPaneId, baseline, 6000);
    expect(hit).not.toBeNull();
  });

  it('Ctrl+Shift+V uses the same canonical pipeline', async () => {
    const baseline = await focusedPaneBaseline(focusedPaneId);
    await refocusXterm();
    await sendChord(['Control', 'Shift', 'v']);
    const hit = await waitForSentinelOnPane(focusedPaneId, baseline, 6000);
    expect(hit).not.toBeNull();
  });

  it('right-click → Paste menu routes through the same handler', async () => {
    const baseline = await focusedPaneBaseline(focusedPaneId);
    await browser.execute((pid) => {
      const el = document.querySelector(`[data-pane-id="${pid}"]`);
      el?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 10, clientY: 10 }));
    }, focusedPaneId);
    // ContextMenu defers its listener via requestAnimationFrame.
    await browser.pause(300);
    const clicked = await browser.execute(() => {
      for (const b of document.querySelectorAll('button')) {
        if (b.textContent.trim().startsWith('Paste')) { b.click(); return true; }
      }
      return false;
    });
    expect(clicked).toBe(true);
    const hit = await waitForSentinelOnPane(focusedPaneId, baseline, 6000);
    expect(hit).not.toBeNull();
  });

  it('drag text → sentinel lands via the plain-text drop path', async () => {
    const baseline = await focusedPaneBaseline(focusedPaneId);
    const dropped = await browser.execute((pid, payload) => {
      const el = document.querySelector(`[data-pane-id="${pid}"]`);
      if (!el) return false;
      const dt = new DataTransfer();
      dt.setData('text/plain', payload);
      el.dispatchEvent(new DragEvent('drop', {
        bubbles: true, cancelable: true, dataTransfer: dt,
      }));
      return true;
    }, focusedPaneId, SENTINEL);
    expect(dropped).toBe(true);
    const hit = await waitForSentinelOnPane(focusedPaneId, baseline, 6000);
    expect(hit).not.toBeNull();
  });
});
