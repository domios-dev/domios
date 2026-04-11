/**
 * Shared helpers for WebdriverIO tests.
 * All UI interactions go through these — single source of truth.
 */

import fs from 'fs';
import path from 'path';
import http from 'http';

// --- API ---

export function readToken() {
  const p = path.join(process.env.HOME, '.codeterm-dev', 'config.toml');
  if (fs.existsSync(p)) {
    const m = fs.readFileSync(p, 'utf8').match(/token\s*=\s*"([^"]+)"/);
    return m?.[1] || '';
  }
  return '';
}

const API_TOKEN = process.env.CODETERM_API_TOKEN || readToken();
const API_PORT = process.env.CODETERM_API_PORT || '7686';

export function api(method, apiPath, body = null) {
  return new Promise((resolve, reject) => {
    const req = http.request({
      hostname: '127.0.0.1', port: parseInt(API_PORT),
      path: `/api${apiPath}`, method,
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

// --- Pane helpers ---

export async function getFirstPane() {
  const panes = await api('GET', '/panes');
  return panes.find(p => p.pty_id);
}

export async function getClaudePane() {
  const panes = await api('GET', '/panes');
  return panes.find(p => p.pty_id && p.agent_type === 'claude');
}

export async function getWorkspaceId() {
  const wss = await api('GET', '/workspaces');
  return wss[0]?.id;
}

export async function setAgentType(paneId, type = 'claude') {
  return api('POST', `/panes/${paneId}/agent-type`, { agent_type: type });
}

export async function injectPrompt(paneId, question = 'Allow?', options = null) {
  const opts = options || [
    { label: 'Yes', value: '1' },
    { label: 'Yes, allow for this session', value: '2' },
    { label: 'No', value: '3' },
  ];
  return api('POST', `/panes/${paneId}/hook/permission`, { question, options: opts });
}

export async function clearPrompt(paneId) {
  return api('POST', `/panes/${paneId}/prompt/clear`);
}

export async function respondPrompt(paneId, value) {
  return api('POST', `/panes/${paneId}/prompt/respond`, { value });
}

export async function getPane(paneId) {
  const panes = await api('GET', '/panes');
  return panes.find(p => p.id === paneId);
}

// --- UI helpers ---

/** Click button by title attribute. Returns true if found and clicked. */
export async function clickButton(title) {
  return browser.execute((t) => {
    const btn = document.querySelector(`button[title="${t}"]`);
    if (btn) { btn.click(); return true; }
    return false;
  }, title);
}

/** Click button by exact text content. */
export async function clickButtonByText(text) {
  return browser.execute((t) => {
    for (const btn of document.querySelectorAll('button')) {
      if (btn.textContent.trim() === t) { btn.click(); return true; }
    }
    return false;
  }, text);
}

/** Check if text exists anywhere in the page. */
export async function pageContains(text) {
  return browser.execute((t) => document.body.innerHTML.includes(t), text);
}

/** Get all visible button labels matching a filter. */
export async function getButtonLabels(filter = null) {
  return browser.execute((f) => {
    const labels = Array.from(document.querySelectorAll('button'))
      .map(b => b.textContent.trim())
      .filter(t => t.length > 0 && t.length < 60);
    if (f) return labels.filter(l => f.includes(l) || l.startsWith(f[0]));
    return labels;
  }, filter);
}

/** Open chat view (handles both states). */
export async function openChatView() {
  const input = await $('textarea[placeholder*="Send"]');
  if (await input.isExisting()) return true;

  const btn = await $('button[title="Switch to chat view"]');
  if (await btn.isExisting()) {
    await btn.click();
    await browser.waitUntil(async () => {
      const ta = await $('textarea[placeholder*="Send"]');
      return await ta.isExisting();
    }, { timeout: 5000, timeoutMsg: 'Chat view did not open' });
    return true;
  }
  return false;
}

/** Close chat view. */
export async function closeChatView() {
  const btn = await $('button[title="Switch to terminal"]');
  if (await btn.isExisting()) await btn.click();
}

/** Wait for prompt buttons to appear in UI. Returns button labels. */
export async function waitForPromptButtons(timeout = 10000) {
  let buttons = [];
  await browser.waitUntil(async () => {
    buttons = await browser.execute(() =>
      Array.from(document.querySelectorAll('button'))
        .map(b => b.textContent.trim())
        .filter(t => ['Yes', 'No'].includes(t) || t.startsWith('Yes,'))
    );
    return buttons.length > 0;
  }, { timeout, timeoutMsg: 'Prompt buttons did not appear' });
  return buttons;
}

/** Count tabs in sidebar. */
export async function countTabs() {
  return browser.execute(() =>
    document.querySelectorAll('button[title="Close tab"]').length
  );
}

/** Wait for pane text to contain pattern (via API). */
export async function waitForScreen(paneId, pattern, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const text = (await api('GET', `/panes/${paneId}/text`)).text || '';
    if (text.includes(pattern)) return text;
    await browser.pause(2000);
  }
  throw new Error(`Pattern "${pattern}" not in pane text within ${timeout}ms`);
}
