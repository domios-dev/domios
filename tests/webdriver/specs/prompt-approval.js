/**
 * Prompt approval UI tests — verify pending_prompt renders in chat view
 * and the full approve/reject flow works end-to-end.
 *
 * Uses the hook/permission endpoint with explicit body to inject prompts
 * without needing agent output in the ring buffer.
 */

import http from 'http';

const API_TOKEN = process.env.CODETERM_API_TOKEN || '7aef7cb945a2445ca05aa058cfd5ef6d';
const API_PORT = process.env.CODETERM_API_PORT || '7686';

// Helper: call CodeTerm REST API
function apiCall(method, path, body = null) {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: '127.0.0.1',
      port: parseInt(API_PORT),
      path: `/api${path}`,
      method,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${API_TOKEN}`,
      },
    };

    const req = http.request(options, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try {
          resolve({ status: res.statusCode, body: JSON.parse(data) });
        } catch {
          resolve({ status: res.statusCode, body: data });
        }
      });
    });
    req.on('error', reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

// Helper: get first pane with a PTY
async function getFirstPane() {
  const { body: panes } = await apiCall('GET', '/panes');
  return panes.find(p => p.pty_id);
}

// Helper: get workspace ID
async function getWorkspaceId() {
  const { body: workspaces } = await apiCall('GET', '/workspaces');
  return workspaces[0]?.id;
}

// Helper: create a new tab
async function createTab(workspaceId) {
  const { body } = await apiCall('POST', `/workspaces/${workspaceId}/tabs`);
  return body;
}

// Helper: inject a permission prompt on a pane via API
async function injectPrompt(paneId, question = 'Do you want to proceed?', options = null) {
  const opts = options || [
    { label: 'Yes', value: '1' },
    { label: 'Yes, allow for this session', value: '2' },
    { label: 'No', value: '3' },
  ];
  return apiCall('POST', `/panes/${paneId}/hook/permission`, { question, options: opts });
}

// Helper: respond to a prompt
async function respondPrompt(paneId, value) {
  return apiCall('POST', `/panes/${paneId}/prompt/respond`, { value });
}

// Helper: get pane data by ID
async function getPane(paneId) {
  const { body: panes } = await apiCall('GET', '/panes');
  return panes.find(p => p.id === paneId);
}

// Helper: open chat view by clicking the chat toggle button
// The button title is "Switch to chat view" (when terminal is shown)
// or "Switch to terminal" (when chat is shown).
async function openChatView() {
  // Check if chat is already open
  const input = await $('textarea[placeholder*="Send"]');
  if (await input.isExisting()) return;

  // Find the "Switch to chat view" button
  const chatBtn = await $('button[title="Switch to chat view"]');
  if (await chatBtn.isExisting()) {
    await chatBtn.click();
    await browser.pause(500);
    const inputAfter = await $('textarea[placeholder*="Send"]');
    await inputAfter.waitForExist({ timeout: 5000 });
  }
}

// Helper: close chat view
async function closeChatView() {
  const termBtn = await $('button[title="Switch to terminal"]');
  if (await termBtn.isExisting()) {
    await termBtn.click();
    await browser.pause(300);
  }
}

// Use executeScript to find text in DOM — avoids stale element issues
async function findTextInPage(searchText) {
  return browser.execute((text) => {
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      null,
    );
    while (walker.nextNode()) {
      if (walker.currentNode.textContent.includes(text)) {
        return true;
      }
    }
    return false;
  }, searchText);
}

// Use executeScript to find prompt option buttons by their text
async function getPromptButtonLabels() {
  return browser.execute(() => {
    const buttons = document.querySelectorAll('button');
    const labels = [];
    for (const btn of buttons) {
      const text = btn.textContent.trim();
      // Prompt option buttons have short labels and are inside the prompt card
      if (['Yes', 'No', 'Allow once', 'Allow for session', 'Deny'].includes(text) ||
          text.startsWith('Yes,')) {
        labels.push(text);
      }
    }
    return labels;
  });
}

// Click a prompt button by its label text
async function clickPromptButton(label) {
  return browser.execute((lbl) => {
    const buttons = document.querySelectorAll('button');
    for (const btn of buttons) {
      if (btn.textContent.trim() === lbl) {
        btn.click();
        return true;
      }
    }
    return false;
  }, label);
}

describe('Prompt Approval in Chat View', () => {
  let paneId;
  let workspaceId;

  before(async () => {
    await browser.pause(3000);
    const pane = await getFirstPane();
    if (pane) paneId = pane.id;
    workspaceId = await getWorkspaceId();
  });

  afterEach(async () => {
    if (paneId) {
      try { await respondPrompt(paneId, '3'); } catch { /* ignore */ }
    }
  });

  it('should have a pane available for testing', async () => {
    expect(paneId).toBeTruthy();
    console.log(`Using pane: ${paneId}`);
  });

  it('should show chat toggle button', async () => {
    // Button title is "Switch to chat view" or "Switch to terminal"
    const buttons = await $$('button[title*="Switch to"]');
    expect(buttons.length).toBeGreaterThan(0);
  });

  it('should open chat view with input area', async () => {
    await openChatView();
    const input = await $('textarea[placeholder*="Send"]');
    expect(await input.isExisting()).toBe(true);
  });

  it('should show prompt card after hook/permission with explicit body', async () => {
    if (!paneId) return;

    await openChatView();

    // Inject prompt
    const result = await injectPrompt(paneId);
    expect(result.body.ok).toBe(true);

    // Wait for ChatView poll (2s interval + some margin)
    await browser.pause(4000);

    // Check for the question text in the DOM
    const found = await findTextInPage('Do you want to proceed?');
    expect(found).toBe(true);
  });

  it('should show option buttons with correct labels', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(4000);

    const labels = await getPromptButtonLabels();
    console.log('Prompt button labels:', labels);
    expect(labels.length).toBeGreaterThanOrEqual(2);
    expect(labels).toContain('Yes');
    expect(labels).toContain('No');
  });

  it('should clear prompt card after clicking Yes', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(4000);

    // Click "Yes"
    const clicked = await clickPromptButton('Yes');
    expect(clicked).toBe(true);
    await browser.pause(2000);

    // Prompt buttons should be gone
    const labelsAfter = await getPromptButtonLabels();
    expect(labelsAfter.length).toBe(0);

    // API confirms prompt cleared
    const pane = await getPane(paneId);
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('should display custom question text and option labels', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId, 'Allow Bash command: rm -rf /tmp/test?', [
      { label: 'Allow once', value: '1' },
      { label: 'Allow for session', value: '2' },
      { label: 'Deny', value: '3' },
    ]);
    await browser.pause(4000);

    const foundQuestion = await findTextInPage('Allow Bash command');
    expect(foundQuestion).toBe(true);

    const labels = await getPromptButtonLabels();
    expect(labels).toContain('Allow once');
    expect(labels).toContain('Deny');
  });
});

describe('Two Panes with Independent Prompts', () => {
  let pane1Id;
  let pane2Id;
  let workspaceId;

  before(async () => {
    await browser.pause(2000);
    workspaceId = await getWorkspaceId();
    const pane1 = await getFirstPane();
    if (pane1) pane1Id = pane1.id;

    if (workspaceId) {
      const tab2 = await createTab(workspaceId);
      pane2Id = tab2.pane_id;
      await browser.pause(3000);
    }
  });

  after(async () => {
    for (const pid of [pane1Id, pane2Id]) {
      if (pid) {
        try { await respondPrompt(pid, '3'); } catch { /* ignore */ }
      }
    }
    if (pane2Id) {
      try { await apiCall('POST', `/panes/${pane2Id}/close`); } catch { /* ignore */ }
    }
  });

  it('should have two distinct panes', async () => {
    expect(pane1Id).toBeTruthy();
    expect(pane2Id).toBeTruthy();
    expect(pane1Id).not.toBe(pane2Id);
    console.log(`Pane 1: ${pane1Id}, Pane 2: ${pane2Id}`);
  });

  it('should set prompts on both panes independently via API', async () => {
    await injectPrompt(pane1Id, 'Pane 1: Allow file read?');
    await injectPrompt(pane2Id, 'Pane 2: Allow network access?');
    await browser.pause(1000);

    const p1 = await getPane(pane1Id);
    const p2 = await getPane(pane2Id);
    expect(p1.pending_prompt).not.toBeNull();
    expect(p2.pending_prompt).not.toBeNull();
    expect(p1.pending_prompt.question).toContain('Pane 1');
    expect(p2.pending_prompt.question).toContain('Pane 2');
  });

  it('approving pane 1 should not affect pane 2 prompt', async () => {
    await injectPrompt(pane1Id, 'Pane 1: run tests?');
    await injectPrompt(pane2Id, 'Pane 2: deploy?');
    await browser.pause(500);

    // Approve pane 1
    await respondPrompt(pane1Id, '1');
    await browser.pause(500);

    const p1 = await getPane(pane1Id);
    const p2 = await getPane(pane2Id);
    expect(p1.pending_prompt).toBeFalsy();
    expect(p2.pending_prompt).not.toBeFalsy();
    expect(p2.pending_prompt.question).toContain('Pane 2');
  });

  it('should show prompt in chat view for the agent pane (pane 1)', async () => {
    // Only pane 1 (the original pane with agent) has chat view support.
    // Pane 2 (API-created, no agent) won't have a chat toggle.
    await injectPrompt(pane1Id, 'Agent pane prompt');
    await browser.pause(500);

    await apiCall('POST', `/panes/${pane1Id}/focus`);
    await browser.pause(500);
    await openChatView();
    await browser.pause(4000);

    const found = await findTextInPage('Agent pane prompt');
    expect(found).toBe(true);

    // Clean up
    await closeChatView();
  });
});

describe('Prompt UI Button Click Flow', () => {
  let paneId;

  before(async () => {
    await browser.pause(1000);
    const pane = await getFirstPane();
    if (pane) paneId = pane.id;
  });

  afterEach(async () => {
    if (paneId) {
      try { await respondPrompt(paneId, '3'); } catch { /* ignore */ }
    }
  });

  it('clicking Yes should clear prompt from both UI and API state', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(4000);

    const clicked = await clickPromptButton('Yes');
    expect(clicked).toBe(true);
    await browser.pause(2000);

    // UI clear
    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);

    // API clear
    const pane = await getPane(paneId);
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('clicking No should also clear the prompt', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(4000);

    const clicked = await clickPromptButton('No');
    expect(clicked).toBe(true);
    await browser.pause(2000);

    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);

    const pane = await getPane(paneId);
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('prompt should not reappear after dismissal even after poll cycles', async () => {
    if (!paneId) return;

    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(4000);

    await clickPromptButton('Yes');
    await browser.pause(1000);

    // Wait through multiple poll cycles (2s each)
    await browser.pause(6000);

    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);
  });
});
