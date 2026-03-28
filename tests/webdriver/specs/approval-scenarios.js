/**
 * Real Claude approval scenarios — tested manually, then automated.
 *
 * Strategy: Claude hooks may not fire in dev (env var issues with prod CodeTerm).
 * So we: 1) wait for prompt on screen, 2) inject via /hook/permission API,
 * 3) verify UI shows buttons, 4) click approve/reject, 5) verify outcome.
 *
 * Run: npx wdio run wdio.conf.js --spec specs/approval-scenarios.js
 */

import fs from 'fs';
import path from 'path';
import http from 'http';

const API_TOKEN = (() => {
  const p = path.join(process.env.HOME, '.codeterm-dev', 'config.toml');
  if (fs.existsSync(p)) {
    const m = fs.readFileSync(p, 'utf8').match(/token\s*=\s*"([^"]+)"/);
    return m?.[1] || '';
  }
  return '';
})();

function api(method, apiPath, body = null) {
  return new Promise((resolve, reject) => {
    const req = http.request({
      hostname: '127.0.0.1', port: 7686, path: `/api${apiPath}`, method,
      headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${API_TOKEN}` },
    }, res => {
      let d = '';
      res.on('data', c => d += c);
      res.on('end', () => { try { resolve(JSON.parse(d)); } catch { resolve(d); } });
    });
    req.on('error', reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

// Wait for prompt to appear on terminal screen
async function waitForScreenPrompt(paneId, timeoutS = 60) {
  for (let i = 0; i < timeoutS; i += 3) {
    await browser.pause(3000);
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    if (screen.includes('Esc to cancel')) return screen;
  }
  throw new Error(`Prompt did not appear on screen within ${timeoutS}s`);
}

// Ensure pending_prompt is set (inject if hooks didn't fire)
async function ensurePromptSet(paneId) {
  const pane = (await api('GET', '/panes')).find(p => p.id === paneId);
  if (pane?.pending_prompt) return pane.pending_prompt;

  // Hooks didn't fire — inject manually from ring buffer
  await api('POST', `/panes/${paneId}/hook/permission`);
  await browser.pause(1000);
  const p2 = (await api('GET', '/panes')).find(p => p.id === paneId);
  return p2?.pending_prompt;
}

// Open chat view, wait for prompt buttons
async function openChatAndWaitForButtons(timeoutS = 20) {
  // Open chat
  const chatBtn = await $('button[title="Switch to chat view"]');
  if (await chatBtn.isExisting()) {
    await chatBtn.click();
    await browser.pause(500);
  }

  // Wait for buttons
  for (let i = 0; i < timeoutS; i += 2) {
    await browser.pause(2000);
    const buttons = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => ['Yes', 'No'].includes(t) || t.startsWith('Yes,'))
    );
    if (buttons.length > 0) return buttons;
  }
  throw new Error('Prompt buttons did not appear');
}

// Click a button by label
async function clickBtn(label) {
  return browser.execute((lbl) => {
    for (const b of document.querySelectorAll('button')) {
      if (b.textContent.trim() === lbl || b.textContent.trim().startsWith(lbl)) {
        b.click(); return true;
      }
    }
    return false;
  }, label);
}

// Clean up: send Escape to cancel any stuck prompt
async function cancelAnyPrompt(paneId) {
  await api('POST', `/panes/${paneId}/send`, { text: '\u001b', enter: false });
  await browser.pause(1000);
  // Also clear pending_prompt via API
  await api('POST', `/panes/${paneId}/prompt/respond`, { value: '3' });
}

// Switch to terminal view
async function switchToTerminal() {
  const btn = await $('button[title="Switch to terminal"]');
  if (await btn.isExisting()) await btn.click();
  await browser.pause(300);
}

// Get first pane with Claude running
async function getClaudePane() {
  const panes = await api('GET', '/panes');
  return panes.find(p => p.pty_id && p.agent_type === 'claude');
}

describe('Case 1: Numbered answer → prompt → approve → file created', () => {
  let paneId;
  const testFile = '/tmp/codeterm-case1-test.txt';

  before(async () => {
    await browser.pause(3000);
    const pane = await getClaudePane();
    if (!pane) throw new Error('No Claude pane found — start Claude first');
    paneId = pane.id;
    try { fs.unlinkSync(testFile); } catch {}
  });

  it('send task with numbered list + file creation', async () => {
    await switchToTerminal();
    await api('POST', `/panes/${paneId}/send`, {
      text: `list 3 design patterns as a numbered list, then create ${testFile} with those names`,
      enter: true,
    });
    const screen = await waitForScreenPrompt(paneId);
    console.log('Screen has prompt + numbered list');
  });

  it('prompt detected (not the design patterns)', async () => {
    const prompt = await ensurePromptSet(paneId);
    if (prompt) {
      const labels = prompt.options.map(o => o.label);
      console.log(`Options: ${JSON.stringify(labels)}`);
      expect(labels.some(l => l === 'Yes' || l.startsWith('Yes'))).toBe(true);
      expect(labels.every(l => !l.toLowerCase().includes('singleton') && !l.toLowerCase().includes('observer'))).toBe(true);
    } else {
      // Hooks may fire later — UI test will verify buttons appear
      console.log('Prompt not in API yet — will verify via UI buttons');
    }
  });

  it('click Yes in chat UI', async () => {
    const buttons = await openChatAndWaitForButtons();
    console.log(`Buttons: ${JSON.stringify(buttons)}`);
    const clicked = await clickBtn('Yes');
    expect(clicked).toBe(true);
    await browser.pause(2000);

    // Buttons gone
    const remaining = await browser.execute(() =>
      Array.from(document.querySelectorAll('button')).map(b => b.textContent.trim()).filter(t => t === 'Yes' || t === 'No')
    );
    expect(remaining.length).toBe(0);
  });

  it('file created', async () => {
    await browser.pause(10000);
    expect(fs.existsSync(testFile)).toBe(true);
    console.log(`File: ${fs.readFileSync(testFile, 'utf8').slice(0, 100)}`);
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    await switchToTerminal();
  });
});

describe('Case 2: Prompt → reject → file NOT created', () => {
  let paneId;
  // Use a different path than /tmp (case 1 may have allowed /tmp for session)
  const testFile = path.join(process.env.HOME, 'codeterm-case2-reject.txt');

  before(async () => {
    // Wait for Claude to be idle from previous test
    await browser.pause(5000);
    const pane = await getClaudePane();
    if (!pane) throw new Error('No Claude pane');
    paneId = pane.id;

    // Wait for idle (? for shortcuts visible)
    for (let i = 0; i < 20; i++) {
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('? for shortcuts')) break;
      await browser.pause(3000);
    }

    try { fs.unlinkSync(testFile); } catch {}
  });

  it('send task and wait for prompt', async () => {
    await switchToTerminal();
    await api('POST', `/panes/${paneId}/send`, {
      text: `create file ${testFile} with content this-should-not-exist`,
      enter: true,
    });
    await waitForScreenPrompt(paneId);
    await ensurePromptSet(paneId);
  });

  it('click No → file NOT created', async () => {
    const buttons = await openChatAndWaitForButtons();
    expect(buttons).toContain('No');
    await clickBtn('No');
    await browser.pause(5000);
    expect(fs.existsSync(testFile)).toBe(false);
    console.log('File correctly not created');
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    await cancelAnyPrompt(paneId);
    await switchToTerminal();
  });
});

describe('Case 3: False positive — numbered list without prompt', () => {
  let paneId;

  before(async () => {
    await browser.pause(5000);
    const pane = await getClaudePane();
    if (!pane) throw new Error('No Claude pane');
    paneId = pane.id;

    // Wait for idle
    for (let i = 0; i < 20; i++) {
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('? for shortcuts')) break;
      await browser.pause(3000);
    }
  });

  it('ask for numbered list (no file action)', async () => {
    await switchToTerminal();
    await api('POST', `/panes/${paneId}/send`, {
      text: 'list 5 common data structures as a numbered list. do not create any files or use any tools.',
      enter: true,
    });
    // Wait for Claude to answer (no prompt expected)
    for (let i = 0; i < 15; i++) {
      await browser.pause(3000);
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('? for shortcuts')) break;
    }
    await browser.pause(3000);
  });

  it('no prompt card in UI', async () => {
    const pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    expect(pane?.pending_prompt).toBeFalsy();

    const chatBtn = await $('button[title="Switch to chat view"]');
    if (await chatBtn.isExisting()) {
      await chatBtn.click();
      await browser.pause(3000);
    }

    const buttons = await browser.execute(() =>
      Array.from(document.querySelectorAll('button')).map(b => b.textContent.trim()).filter(t => ['Yes','No'].includes(t))
    );
    expect(buttons.length).toBe(0);
    console.log('No false positive prompt');
  });

  after(async () => { await switchToTerminal(); });
});
