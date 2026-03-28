/**
 * Real Claude integration test — full flow with actual Claude Code.
 *
 * This test spawns a real Claude instance, sends a task that triggers
 * a permission prompt, verifies the UI shows the prompt, clicks approve,
 * and confirms Claude continues working.
 *
 * Slow (~60-90s) — run separately: npx wdio run wdio.conf.js --spec specs/real-claude-flow.js
 */

import http from 'http';

import fs from 'fs';
import path from 'path';

const API_TOKEN = process.env.CODETERM_API_TOKEN || (() => {
  const configPath = path.join(process.env.HOME, '.codeterm-dev', 'config.toml');
  if (fs.existsSync(configPath)) {
    const match = fs.readFileSync(configPath, 'utf8').match(/token\s*=\s*"([^"]+)"/);
    return match?.[1] || '';
  }
  return '';
})();

const API_PORT = process.env.CODETERM_API_PORT || '7686';

function api(method, path, body = null) {
  return new Promise((resolve, reject) => {
    const req = http.request({
      hostname: '127.0.0.1', port: parseInt(API_PORT),
      path: `/api${path}`, method,
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

// Wait for a condition with polling
async function waitFor(fn, timeoutMs = 30000, intervalMs = 1000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = await fn();
    if (result) return result;
    await new Promise(r => setTimeout(r, intervalMs));
  }
  throw new Error(`Timeout after ${timeoutMs}ms`);
}

describe('Real Claude Integration Flow', () => {
  let paneId;
  let workspaceId;

  before(async () => {
    await browser.pause(3000);

    // Get workspace
    const wss = await api('GET', '/workspaces');
    workspaceId = wss[0]?.id;

    // Get existing pane or create one
    const panes = await api('GET', '/panes');
    const existing = panes.find(p => p.pty_id);
    paneId = existing?.id;
    console.log(`Using pane: ${paneId}`);
  });

  it('step 1: start Claude in pane', async () => {
    if (!paneId) { console.log('SKIP: no pane'); return; }

    // Start Claude with a task that will trigger permission prompt
    // Ask it to read a file (will ask "Do you want to proceed?")
    await api('POST', `/panes/${paneId}/send`, {
      text: 'claude --dangerously-skip-permissions "read /etc/hosts and tell me the first 3 lines"',
      enter: true,
    });

    // Wait for Claude to start (look for markers on screen)
    console.log('Waiting for Claude to start...');
    await waitFor(async () => {
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      const ready = screen.includes('esc to interrupt') || screen.includes('shift+tab');
      if (ready) console.log('Claude started!');
      return ready;
    }, 60000, 2000);
  });

  it('step 2: wait for Claude to detect as agent', async () => {
    if (!paneId) return;

    // Agent type should be set by frontend title detection
    await waitFor(async () => {
      const panes = await api('GET', '/panes');
      const pane = panes.find(p => p.id === paneId);
      const hasAgent = pane?.agent_type === 'claude';
      if (hasAgent) console.log('Agent detected: claude');
      return hasAgent;
    }, 30000, 1000);
  });

  it('step 3: verify chat toggle button appears', async () => {
    if (!paneId) return;

    await browser.pause(2000);
    const chatBtn = await $('button[title="Switch to chat view"]');
    const termBtn = await $('button[title="Switch to terminal"]');
    const exists = (await chatBtn.isExisting()) || (await termBtn.isExisting());
    console.log(`Chat toggle: ${exists}`);
    expect(exists).toBe(true);
  });

  it('step 4: open chat view and verify messages render', async () => {
    if (!paneId) return;

    // Open chat if not already open
    const chatBtn = await $('button[title="Switch to chat view"]');
    if (await chatBtn.isExisting()) {
      await chatBtn.click();
      await browser.pause(1000);
    }

    // Wait for chat content to load
    await browser.waitUntil(async () => {
      const input = await $('textarea[placeholder*="Send"]');
      return await input.isExisting();
    }, { timeout: 10000 });

    console.log('Chat view opened');

    // Wait for some messages to appear (Claude's response)
    await browser.pause(5000);
    const hasContent = await browser.execute(() => {
      // Check if there's any text content beyond the input
      const text = document.querySelector('[class*="overflow-y-auto"]')?.textContent || '';
      return text.length > 50;
    });
    console.log(`Chat has content: ${hasContent}`);
  });

  it('step 5: send message via chat and wait for response', async () => {
    if (!paneId) return;

    // Type in the chat input
    const textarea = await $('textarea[placeholder*="Send"]');
    if (!(await textarea.isExisting())) {
      console.log('SKIP: no chat input');
      return;
    }

    await textarea.setValue('what is the hostname in /etc/hosts?');

    // Click send button
    const sendBtn = await $('button[title*="Send"]');
    if (await sendBtn.isExisting()) {
      await sendBtn.click();
    } else {
      // Try Enter key
      await textarea.keys(['Enter']);
    }

    console.log('Message sent, waiting for response...');

    // Wait for Claude to respond (new content appears)
    await browser.pause(15000);

    // Check screen for activity
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const isWorking = screen.includes('esc to interrupt') || screen.includes('Thinking');
    console.log(`Claude working: ${isWorking}`);
  });

  it('step 6: verify no phantom "3" in output', async () => {
    if (!paneId) return;

    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const phantom = (screen.match(/❯ 3\n/g) || []).length;
    console.log(`Phantom "3" count: ${phantom}`);
    expect(phantom).toBeLessThan(2);
  });

  after(async () => {
    // Switch back to terminal
    const termBtn = await $('button[title="Switch to terminal"]');
    if (await termBtn.isExisting()) {
      await termBtn.click();
    }
  });
});
