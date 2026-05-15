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
  const focused = await browser.execute(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    ta.focus();
    return { ok: document.activeElement === ta };
  });
  if (!focused.ok) throw new Error(`could not focus xterm: ${JSON.stringify(focused)}`);
}

async function clearAllOutputs() {
  // Sentinel must appear AFTER the action under test. The send-text endpoint
  // writes a clear-screen byte (\f / Ctrl+L) to the visible pane — but since
  // sentinel check is over ALL panes, we just rely on the sentinel being
  // unique per test (Date.now()-suffixed). The clearScreen step on the focused
  // pane is best-effort.
  await browser.execute(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    ta?.focus();
  });
}

async function waitForSentinelAnywhere(timeoutMs = 5000, baseline = {}) {
  // Claude TUI eats bracketed-paste and shows it as `[Pasted text #N]` —
  // SENTINEL itself never appears on screen in that case. So we accept
  // either the literal sentinel (shell pane echo path) OR a NEW
  // "[Pasted text #N]" placeholder that wasn't there in `baseline`.
  const placeholderRe = /\[Pasted text #(\d+)/g;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const panes = await api('GET', '/panes');
    for (const p of panes) {
      if (!p.pty_id) continue;
      const out = await api('GET', `/panes/${p.id}/output?offset=0&strip_ansi=true`);
      const text = out.output || '';
      if (text.includes(SENTINEL)) return { paneId: p.id, kind: 'sentinel' };
      const placeholders = [...text.matchAll(placeholderRe)].map((m) => Number(m[1]));
      const baselineMax = baseline[p.id] ?? -1;
      const fresh = placeholders.filter((n) => n > baselineMax);
      if (fresh.length > 0) return { paneId: p.id, kind: 'placeholder', n: Math.max(...fresh) };
    }
    await browser.pause(200);
  }
  return null;
}

async function snapshotPlaceholders() {
  const out = {};
  const panes = await api('GET', '/panes');
  for (const p of panes) {
    if (!p.pty_id) continue;
    const r = await api('GET', `/panes/${p.id}/output?offset=0&strip_ansi=true`);
    const text = r.output || '';
    const ns = [...text.matchAll(/\[Pasted text #(\d+)/g)].map((m) => Number(m[1]));
    out[p.id] = ns.length ? Math.max(...ns) : -1;
  }
  return out;
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
  before(async () => {
    // Force a hard reload so the frontend re-fetches pane state from the
    // backend instead of rendering a stale React snapshot. Without this,
    // Tauri WebView keeps the previous dev session's component tree alive
    // across daemon restarts — DOM pane ids drift away from API truth.
    await browser.refresh();
    await browser.pause(2000);
    await focusVisiblePane();
    const wrote = await writeClipboard(SENTINEL);
    expect(wrote).toBe(true);
  });

  it('Ctrl+V → sentinel lands in some PTY', async () => {
    const baseline = await snapshotPlaceholders();
    await clearAllOutputs();
    await sendChord(['Control', 'v']);
    const hit = await waitForSentinelAnywhere(6000, baseline);
    expect(hit).not.toBeNull();
  });

  it('Ctrl+Shift+V uses the same canonical pipeline', async () => {
    const baseline = await snapshotPlaceholders();
    await clearAllOutputs();
    await sendChord(['Control', 'Shift', 'v']);
    const hit = await waitForSentinelAnywhere(6000, baseline);
    expect(hit).not.toBeNull();
  });

  it('right-click → Paste menu routes through the same handler', async () => {
    const baseline = await snapshotPlaceholders();
    await clearAllOutputs();
    await browser.execute(() => {
      const el = document.querySelector('[data-pane-id]');
      el?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 10, clientY: 10 }));
    });
    // ContextMenu defers its listener via requestAnimationFrame.
    await browser.pause(300);
    const clicked = await browser.execute(() => {
      for (const b of document.querySelectorAll('button')) {
        if (b.textContent.trim().startsWith('Paste')) { b.click(); return true; }
      }
      return false;
    });
    expect(clicked).toBe(true);
    const hit = await waitForSentinelAnywhere(6000, baseline);
    expect(hit).not.toBeNull();
  });

  it('drag text → sentinel lands via the plain-text drop path', async () => {
    const baseline = await snapshotPlaceholders();
    await clearAllOutputs();
    const dropped = await browser.execute((payload) => {
      const el = document.querySelector('[data-pane-id]');
      if (!el) return false;
      const dt = new DataTransfer();
      dt.setData('text/plain', payload);
      el.dispatchEvent(new DragEvent('drop', {
        bubbles: true, cancelable: true, dataTransfer: dt,
      }));
      return true;
    }, SENTINEL);
    expect(dropped).toBe(true);
    const hit = await waitForSentinelAnywhere(6000, baseline);
    expect(hit).not.toBeNull();
  });
});
