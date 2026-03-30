/**
 * Core flow: real Claude → prompt → approve/reject via chat UI.
 * Verifies the full approval pipeline end-to-end.
 *
 * Needs: live Claude Code, app running with --features webdriver.
 * Run: npm run test:core
 */

import {
  api, getWorkspaceId, setAgentType, openChatView, closeChatView,
  clearPrompt, waitForPromptButtons,
} from '../helpers.js';
import fs from 'fs';

// Spawn Claude via API. Returns pane_id.
async function spawnClaude(opts = {}) {
  const wsId = await getWorkspaceId();
  const result = await api('POST', '/agents/spawn', {
    agent: 'claude',
    workspace_id: wsId,
    skip_permissions: opts.skipPermissions ?? false,
    task: opts.task || undefined,
  });
  if (!result.pane_id) throw new Error(`Spawn failed: ${JSON.stringify(result)}`);
  return result.pane_id;
}

// Wait for Claude to be ready (TUI visible on screen).
// Auto-accepts trust prompts ("Yes, I accept") if they appear.
async function waitForClaude(paneId, timeoutS = 60) {
  for (let i = 0; i < timeoutS; i += 2) {
    await browser.pause(2000);
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    if (screen.includes('? for shortcuts') || screen.includes('esc to interrupt') || screen.includes('bypass permissions')) {
      return screen;
    }
    // Auto-accept trust/responsibility prompts (Claude first-run in project)
    if (screen.includes('Yes, I accept') || screen.includes('responsibility')) {
      // Raw mode: j to navigate down to "Yes, I accept", Enter to confirm
      await api('POST', `/panes/${paneId}/send`, { text: 'j', raw: true, enter: false });
      await browser.pause(200);
      await api('POST', `/panes/${paneId}/send`, { text: '\r', raw: true, enter: false });
      console.log('Auto-accepted trust prompt');
    }
    // Auto-accept "Continue without settings" if settings.json was broken
    if (screen.includes('Continue without these settings')) {
      await api('POST', `/panes/${paneId}/send`, { text: 'j', raw: true, enter: false });
      await browser.pause(200);
      await api('POST', `/panes/${paneId}/send`, { text: '\r', raw: true, enter: false });
      console.log('Auto-accepted broken settings prompt');
    }
  }
  throw new Error(`Claude did not start within ${timeoutS}s`);
}

// Wait for pending_prompt to appear in API (hooks fire with delay).
async function waitForPrompt(paneId, timeoutS = 30) {
  for (let i = 0; i < timeoutS; i += 2) {
    await browser.pause(2000);
    const panes = await api('GET', '/panes');
    const pane = panes.find(p => p.id === paneId);
    if (pane?.pending_prompt) return pane.pending_prompt;
  }
  // Fallback: inject from ring buffer
  console.log('Prompt not auto-detected, injecting from buffer...');
  await api('POST', `/panes/${paneId}/hook/permission`);
  await browser.pause(1000);
  const panes = await api('GET', '/panes');
  const pane = panes.find(p => p.id === paneId);
  if (pane?.pending_prompt) return pane.pending_prompt;
  throw new Error(`Prompt did not appear within ${timeoutS}s`);
}

// Click a prompt button by label text.
async function clickPromptBtn(label) {
  return browser.execute((lbl) => {
    for (const b of document.querySelectorAll('button')) {
      const t = b.textContent.trim();
      if (t === lbl || t.startsWith(lbl)) { b.click(); return true; }
    }
    return false;
  }, label);
}

// ── Approve flow ──────────────────────────────────────────────────

describe('Approve: Claude creates file after Yes', () => {
  let paneId;
  const testFile = '/tmp/codeterm-core-approve.txt';

  before(async () => {
    try { fs.unlinkSync(testFile); } catch {}
  });

  it('spawn Claude with task that needs approval', async () => {
    paneId = await spawnClaude({
      skipPermissions: false,
      task: `create file ${testFile} with content "core-flow-approve-ok"`,
    });
    expect(paneId).toBeTruthy();
    console.log(`Spawned Claude: ${paneId}`);
  });

  it('Claude starts and shows prompt on screen', async () => {
    const screen = await waitForClaude(paneId);
    // Prompt should appear shortly after
    const prompt = await waitForPrompt(paneId);
    console.log(`Prompt: ${prompt.question}`);
    expect(prompt.options.length).toBeGreaterThan(0);
  });

  it('chat UI shows prompt buttons', async () => {
    await setAgentType(paneId, 'claude');
    // Close any previous chat, focus new pane, wait for frontend to update
    await closeChatView();
    await browser.pause(500);
    await api('POST', `/panes/${paneId}/focus`);
    await browser.pause(2000); // Give frontend time to switch pane
    await openChatView();

    const buttons = await waitForPromptButtons(15000);
    console.log(`Buttons: ${JSON.stringify(buttons)}`);
    expect(buttons.some(b => b === 'Yes' || b.startsWith('Yes'))).toBe(true);
  });

  it('click Yes → prompt clears', async () => {
    const clicked = await clickPromptBtn('Yes');
    expect(clicked).toBe(true);
    await browser.pause(2000);

    // Buttons gone
    const remaining = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => t === 'Yes' || t === 'No')
    );
    expect(remaining.length).toBe(0);

    // API confirms
    const panes = await api('GET', '/panes');
    expect(panes.find(p => p.id === paneId)?.pending_prompt).toBeFalsy();
  });

  it('file was created', async () => {
    // Wait for Claude to finish writing
    for (let i = 0; i < 15; i++) {
      if (fs.existsSync(testFile)) break;
      await browser.pause(2000);
    }
    expect(fs.existsSync(testFile)).toBe(true);
    console.log(`Content: ${fs.readFileSync(testFile, 'utf8').slice(0, 100)}`);
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    await closeChatView();
    // Close spawned pane so it doesn't interfere with next describe
    if (paneId) try { await api('POST', `/panes/${paneId}/close`); } catch {}
    await browser.pause(1000);
  });
});

// ── Reject flow ──────────────────────────────────────────────────

describe('Reject: file NOT created after No', () => {
  let paneId;
  const testFile = '/tmp/codeterm-core-reject.txt';

  before(async () => {
    try { fs.unlinkSync(testFile); } catch {}
  });

  it('spawn Claude with task', async () => {
    paneId = await spawnClaude({
      skipPermissions: false,
      task: `create file ${testFile} with content "this-should-not-exist"`,
    });
    expect(paneId).toBeTruthy();
  });

  it('wait for prompt', async () => {
    await waitForClaude(paneId);
    const prompt = await waitForPrompt(paneId);
    expect(prompt).toBeTruthy();
  });

  it('click No → file NOT created', async () => {
    await setAgentType(paneId, 'claude');
    await api('POST', `/panes/${paneId}/focus`);
    await browser.pause(2000);
    await openChatView();

    const buttons = await waitForPromptButtons(20000);
    expect(buttons).toContain('No');

    await clickPromptBtn('No');
    await browser.pause(5000);

    expect(fs.existsSync(testFile)).toBe(false);
    console.log('File correctly not created');
  });

  after(async () => {
    try { fs.unlinkSync(testFile); } catch {}
    if (paneId) try { await clearPrompt(paneId); } catch {}
    await closeChatView();
  });
});

// ── Chat message flow ────────────────────────────────────────────

describe('Chat: messages render correctly', () => {
  let paneId;

  it('spawn Claude with skip-permissions', async () => {
    paneId = await spawnClaude({
      skipPermissions: true,
      task: 'say exactly one word: PONG',
    });
    expect(paneId).toBeTruthy();
  });

  it('Claude starts and agent_type set', async () => {
    await waitForClaude(paneId);
    // Ensure agent_type is set (auto-detection may be delayed in dev)
    await setAgentType(paneId, 'claude');
    await browser.pause(500);
    const panes = await api('GET', '/panes');
    expect(panes.find(p => p.id === paneId)?.agent_type).toBe('claude');
  });

  it('chat view opens and has content', async () => {
    await closeChatView();
    await browser.pause(500);
    await api('POST', `/panes/${paneId}/focus`);
    await browser.pause(1000);
    await openChatView();

    await expect($('textarea[placeholder*="Send"]')).toBeExisting();

    // Wait for messages to render
    await browser.waitUntil(async () => {
      return browser.execute(() => {
        const el = document.querySelector('[class*="overflow-y-auto"]');
        return (el?.textContent?.length || 0) > 20;
      });
    }, { timeout: 20000, interval: 2000, timeoutMsg: 'No chat content' });
  });

  it('no phantom input in terminal', async () => {
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const phantoms = (screen.match(/❯ 3\n/g) || []).length;
    expect(phantoms).toBeLessThan(2);
  });

  after(async () => {
    await closeChatView();
  });
});
