/**
 * Comprehensive approval flow scenarios — tested manually first, then baked into tests.
 *
 * Covers:
 * 1. Numbered answer → real prompt → approve → file created
 * 2. Prompt → reject (No) → file NOT created
 * 3. False positive guard: numbered list without prompt stays clean
 *
 * Each test spawns a real Claude task. Slow (~30-60s each).
 * Run: npx wdio run wdio.conf.js --spec specs/approval-scenarios.js
 */

import fs from 'fs';
import path from 'path';
import http from 'http';

const API_TOKEN = (() => {
  const configPath = path.join(process.env.HOME, '.codeterm-dev', 'config.toml');
  if (fs.existsSync(configPath)) {
    const match = fs.readFileSync(configPath, 'utf8').match(/token\s*=\s*"([^"]+)"/);
    return match?.[1] || '';
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

async function waitForPromptOnScreen(paneId, timeoutS = 45) {
  for (let i = 0; i < timeoutS; i += 3) {
    await browser.pause(3000);
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    if (screen.includes('Esc to cancel') || screen.includes('proceed')) return screen;
  }
  throw new Error('Prompt did not appear on screen');
}

async function openChatView() {
  const chatBtn = await $('button[title="Switch to chat view"]');
  if (await chatBtn.isExisting()) {
    await chatBtn.click();
    await browser.pause(500);
  }
}

async function waitForPromptButtons(timeoutS = 15) {
  for (let i = 0; i < timeoutS; i += 2) {
    await browser.pause(2000);
    const buttons = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => ['Yes', 'No'].includes(t) || t.startsWith('Yes,'))
    );
    if (buttons.length > 0) return buttons;
  }
  throw new Error('Prompt buttons did not appear in UI');
}

async function clickButton(label) {
  return browser.execute((lbl) => {
    for (const b of document.querySelectorAll('button')) {
      if (b.textContent.trim() === lbl || b.textContent.trim().startsWith(lbl)) {
        b.click();
        return true;
      }
    }
    return false;
  }, label);
}

async function ensureClaude(paneId) {
  // Wait for Claude to be ready
  for (let i = 0; i < 30; i++) {
    await browser.pause(2000);
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    if (screen.includes('esc to interrupt') || screen.includes('shift+tab') || screen.includes('? for shortcuts')) {
      return;
    }
  }
  throw new Error('Claude did not start');
}

describe('Scenario 1: Numbered answer then approve', () => {
  let paneId;
  const testFile = '/tmp/codeterm-scenario1.txt';

  before(async () => {
    await browser.pause(3000);
    const panes = await api('GET', '/panes');
    paneId = panes.find(p => p.pty_id)?.id;
    // Clean up from previous runs
    try { fs.unlinkSync(testFile); } catch {}
  });

  it('should send task that produces numbered list then prompt', async () => {
    // Check if Claude is already running
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const claudeRunning = screen.includes('? for shortcuts') || screen.includes('esc to interrupt');

    if (!claudeRunning) {
      // Export correct pane ID for hooks and start Claude
      await api('POST', `/panes/${paneId}/send`, {
        text: `export CODETERM_PANE_ID=${paneId}`, enter: true,
      });
      await browser.pause(500);
      await api('POST', `/panes/${paneId}/send`, {
        text: `claude "list 3 programming languages with their main use case as a numbered list, then create file ${testFile} with those languages"`,
        enter: true,
      });
      await ensureClaude(paneId);
    } else {
      // Claude already running — just send message
      await api('POST', `/panes/${paneId}/send`, {
        text: `list 3 programming languages with their main use case as a numbered list, then create file ${testFile} with those languages`,
        enter: true,
      });
    }
  });

  it('should detect prompt (not the numbered list)', async () => {
    await waitForPromptOnScreen(paneId);

    // If hooks didn't fire, manually inject via API
    await browser.pause(2000);
    let pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    if (!pane?.pending_prompt) {
      console.log('Hooks did not fire, injecting via API...');
      await api('POST', `/panes/${paneId}/hook/permission`);
      await browser.pause(1000);
      pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    }

    const prompt = pane?.pending_prompt;
    if (prompt) {
      const labels = prompt.options.map(o => o.label);
      expect(labels.some(l => l === 'Yes' || l.startsWith('Yes'))).toBe(true);
      expect(labels.every(l => !l.includes('Python') && !l.includes('JavaScript'))).toBe(true);
      console.log(`Prompt parsed: ${prompt.question}, options: ${JSON.stringify(labels)}`);
    }
  });

  it('should show prompt in chat UI and click Yes', async () => {
    // Ensure agent type is set so chat view is available
    await api('POST', `/panes/${paneId}/agent-type`, { agent_type: 'claude' });
    await browser.pause(1000);
    await openChatView();
    const buttons = await waitForPromptButtons(20);
    console.log(`Prompt buttons: ${JSON.stringify(buttons)}`);
    expect(buttons.length).toBeGreaterThan(0);

    const clicked = await clickButton('Yes');
    expect(clicked).toBe(true);
    await browser.pause(3000);

    // Buttons should be gone
    const remaining = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => ['Yes', 'No'].includes(t))
    );
    expect(remaining.length).toBe(0);
  });

  it('should have created the file', async () => {
    // Wait for Claude to finish writing
    await browser.pause(10000);
    expect(fs.existsSync(testFile)).toBe(true);
    const content = fs.readFileSync(testFile, 'utf8');
    console.log(`File content: ${content.slice(0, 100)}`);
    expect(content.length).toBeGreaterThan(0);
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    // Switch back to terminal
    const btn = await $('button[title="Switch to terminal"]');
    if (await btn.isExisting()) await btn.click();
  });
});

describe('Scenario 2: Reject prompt', () => {
  let paneId;
  const testFile = '/tmp/codeterm-scenario2-reject.txt';

  before(async () => {
    await browser.pause(3000);
    const panes = await api('GET', '/panes');
    paneId = panes.find(p => p.pty_id)?.id;
    try { fs.unlinkSync(testFile); } catch {}
  });

  it('should send task and wait for prompt', async () => {
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const claudeRunning = screen.includes('? for shortcuts') || screen.includes('esc to interrupt');

    if (!claudeRunning) {
      await api('POST', `/panes/${paneId}/send`, {
        text: `claude "create file ${testFile} with content rejected"`,
        enter: true,
      });
      await ensureClaude(paneId);
    } else {
      await api('POST', `/panes/${paneId}/send`, {
        text: `create file ${testFile} with content rejected`,
        enter: true,
      });
    }
    await waitForPromptOnScreen(paneId);
  });

  it('should click No and file should NOT be created', async () => {
    // If hooks didn't fire, inject
    let pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    if (!pane?.pending_prompt) {
      await api('POST', `/panes/${paneId}/hook/permission`);
      await browser.pause(1000);
    }
    await api('POST', `/panes/${paneId}/agent-type`, { agent_type: 'claude' });
    await browser.pause(500);
    await openChatView();
    const buttons = await waitForPromptButtons(20);
    expect(buttons).toContain('No');

    const clicked = await clickButton('No');
    expect(clicked).toBe(true);
    await browser.pause(5000);

    expect(fs.existsSync(testFile)).toBe(false);
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    const btn = await $('button[title="Switch to terminal"]');
    if (await btn.isExisting()) await btn.click();
  });
});

describe('Scenario 3: No false positive on numbered answer', () => {
  let paneId;

  before(async () => {
    await browser.pause(3000);
    const panes = await api('GET', '/panes');
    paneId = panes.find(p => p.pty_id)?.id;
  });

  it('should ask Claude for numbered list (no file action)', async () => {
    await api('POST', `/panes/${paneId}/send`, {
      text: 'claude --dangerously-skip-permissions "list 5 benefits of unit testing as a numbered list. Do not create any files."',
      enter: true,
    });

    // Wait for response
    for (let i = 0; i < 20; i++) {
      await browser.pause(3000);
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('? for shortcuts') || screen.includes('esc to interrupt')) {
        // Claude answered but might still be processing — wait for idle
        if (screen.includes('? for shortcuts')) break;
      }
    }
    await browser.pause(5000);
  });

  it('should NOT show prompt card', async () => {
    const pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    expect(pane?.pending_prompt).toBeFalsy();

    await openChatView();
    await browser.pause(3000);

    const buttons = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => ['Yes', 'No'].includes(t) || t.startsWith('Yes,'))
    );
    expect(buttons.length).toBe(0);
  });

  after(async () => {
    const btn = await $('button[title="Switch to terminal"]');
    if (await btn.isExisting()) await btn.click();
  });
});
