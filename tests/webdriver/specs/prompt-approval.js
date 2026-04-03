/**
 * Prompt approval UI tests — verify pending_prompt renders in chat view
 * and the full approve/reject flow works end-to-end.
 *
 * Uses API for setup (inject prompt), WebDriver for UI verification.
 * Self-contained: creates own tabs, cleans up after.
 */

import {
  api, getFirstPane, getWorkspaceId, setAgentType,
  injectPrompt, clearPrompt, openChatView, closeChatView,
  waitForPromptButtons, clickButton, clickButtonByText, pageContains,
  countTabs,
} from '../helpers.js';

// Find prompt question text in DOM
async function findTextInPage(text) {
  return browser.execute((t) => document.body.innerHTML.includes(t), text);
}

// Get prompt button labels (Yes/No/Allow/Deny)
async function getPromptButtonLabels() {
  return browser.execute(() =>
    Array.from(document.querySelectorAll('button'))
      .map(b => b.textContent.trim())
      .filter(t =>
        ['Yes', 'No', 'Allow once', 'Allow for session', 'Deny'].includes(t) ||
        t.startsWith('Yes,')
      )
  );
}

// Click a prompt button by label
async function clickPromptButton(label) {
  return browser.execute((lbl) => {
    for (const btn of document.querySelectorAll('button')) {
      if (btn.textContent.trim() === lbl) { btn.click(); return true; }
    }
    return false;
  }, label);
}

describe('Prompt Approval in Chat View', () => {
  let paneId;

  before(async () => {
    await browser.pause(3000);
    const pane = await getFirstPane();
    paneId = pane?.id;

    // Set agent_type so chat view toggle appears
    if (paneId) {
      await setAgentType(paneId);
      await browser.pause(1000);
    }
  });

  afterEach(async () => {
    if (paneId) {
      try { await clearPrompt(paneId); } catch {}
    }
  });

  it('should have a pane available for testing', async () => {
    expect(paneId).toBeTruthy();
  });

  it('should show chat toggle button', async () => {
    await expect($('button[title*="Switch to"]')).toBeExisting();
  });

  it('should open chat view with input area', async () => {
    await openChatView();
    await expect($('textarea[placeholder*="Send"]')).toBeExisting();
  });

  it('should show prompt card after inject', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);

    // Wait for ChatView poll to pick up the prompt
    await browser.waitUntil(async () => await findTextInPage('Allow?'), {
      timeout: 8000, interval: 500, timeoutMsg: 'Prompt question did not appear in UI',
    });
  });

  it('should show option buttons with correct labels', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);
    await browser.pause(3000); // wait for ChatView poll to pick up

    await browser.waitUntil(async () => {
      const labels = await getPromptButtonLabels();
      return labels.includes('Yes') && labels.includes('No');
    }, { timeout: 15000, interval: 500, timeoutMsg: 'Yes/No buttons did not appear' });
  });

  it('should clear prompt card after clicking Yes', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);

    await browser.waitUntil(async () => {
      const labels = await getPromptButtonLabels();
      return labels.includes('Yes');
    }, { timeout: 8000, interval: 500, timeoutMsg: 'Yes button did not appear' });

    await clickPromptButton('Yes');
    await browser.pause(1000);

    // Buttons should be gone
    const labelsAfter = await getPromptButtonLabels();
    expect(labelsAfter.length).toBe(0);

    // API confirms prompt cleared
    const pane = await (async () => {
      const panes = await api('GET', '/panes');
      return panes.find(p => p.id === paneId);
    })();
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('should display custom question and option labels', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId, 'Allow Bash command: rm -rf /tmp/test?', [
      { label: 'Allow once', value: '1' },
      { label: 'Allow for session', value: '2' },
      { label: 'Deny', value: '3' },
    ]);

    await browser.waitUntil(async () => await findTextInPage('Allow Bash command'), {
      timeout: 8000, interval: 500, timeoutMsg: 'Custom question did not appear',
    });

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
    pane1Id = pane1?.id;

    // Create second tab
    if (workspaceId) {
      const tab2 = await api('POST', `/workspaces/${workspaceId}/tabs`);
      pane2Id = tab2.pane_id;
      await browser.pause(2000);
    }

    // Set agent_type on both
    if (pane1Id) await setAgentType(pane1Id);
    if (pane2Id) await setAgentType(pane2Id);
    await browser.pause(500);
  });

  after(async () => {
    for (const pid of [pane1Id, pane2Id]) {
      if (pid) try { await clearPrompt(pid); } catch {}
    }
    // Close extra tab
    if (pane2Id) {
      try { await api('POST', `/panes/${pane2Id}/close`); } catch {}
      await browser.pause(500);
    }
  });

  it('should have two distinct panes', async () => {
    expect(pane1Id).toBeTruthy();
    expect(pane2Id).toBeTruthy();
    expect(pane1Id).not.toBe(pane2Id);
  });

  it('should set prompts on both panes independently via API', async () => {
    await injectPrompt(pane1Id, 'Pane 1: Allow file read?');
    await injectPrompt(pane2Id, 'Pane 2: Allow network access?');
    await browser.pause(500);

    const panes = await api('GET', '/panes');
    const p1 = panes.find(p => p.id === pane1Id);
    const p2 = panes.find(p => p.id === pane2Id);
    expect(p1.pending_prompt).not.toBeNull();
    expect(p2.pending_prompt).not.toBeNull();
    expect(p1.pending_prompt.question).toContain('Pane 1');
    expect(p2.pending_prompt.question).toContain('Pane 2');
  });

  it('approving pane 1 should not affect pane 2 prompt', async () => {
    await injectPrompt(pane1Id, 'Pane 1: run tests?');
    await injectPrompt(pane2Id, 'Pane 2: deploy?');
    await browser.pause(500);

    // Clear pane 1 (simulates approve without PTY side effects)
    await clearPrompt(pane1Id);
    await browser.pause(500);

    const panes = await api('GET', '/panes');
    const p1 = panes.find(p => p.id === pane1Id);
    const p2 = panes.find(p => p.id === pane2Id);
    expect(p1.pending_prompt).toBeFalsy();
    expect(p2.pending_prompt).not.toBeFalsy();
    expect(p2.pending_prompt.question).toContain('Pane 2');
  });

  it('should show prompt in chat view for focused pane', async () => {
    await injectPrompt(pane1Id, 'Agent pane prompt');
    await browser.pause(500);

    await api('POST', `/panes/${pane1Id}/focus`);
    await browser.pause(500);
    await openChatView();

    await browser.waitUntil(async () => await findTextInPage('Agent pane prompt'), {
      timeout: 8000, interval: 500, timeoutMsg: 'Prompt did not appear in chat for focused pane',
    });

    await closeChatView();
  });
});

describe('Prompt UI Button Click Flow', () => {
  let paneId;

  before(async () => {
    await browser.pause(1000);
    const pane = await getFirstPane();
    paneId = pane?.id;

    if (paneId) {
      await setAgentType(paneId);
      await browser.pause(500);
    }
  });

  afterEach(async () => {
    if (paneId) try { await clearPrompt(paneId); } catch {}
  });

  it('clicking Yes should clear prompt from both UI and API', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);

    await browser.waitUntil(async () => {
      const labels = await getPromptButtonLabels();
      return labels.includes('Yes');
    }, { timeout: 8000, interval: 500, timeoutMsg: 'Yes button did not appear' });

    await clickPromptButton('Yes');
    await browser.pause(1000);

    // UI clear
    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);

    // API clear
    const panes = await api('GET', '/panes');
    const pane = panes.find(p => p.id === paneId);
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('clicking No should also clear the prompt', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);

    await browser.waitUntil(async () => {
      const labels = await getPromptButtonLabels();
      return labels.includes('No');
    }, { timeout: 8000, interval: 500, timeoutMsg: 'No button did not appear' });

    await clickPromptButton('No');
    await browser.pause(1000);

    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);

    const panes = await api('GET', '/panes');
    const pane = panes.find(p => p.id === paneId);
    expect(pane.pending_prompt).toBeFalsy();
  });

  it('prompt should not reappear after dismissal', async () => {
    if (!paneId) return;
    await openChatView();
    await injectPrompt(paneId);

    await browser.waitUntil(async () => {
      const labels = await getPromptButtonLabels();
      return labels.includes('Yes');
    }, { timeout: 8000, interval: 500, timeoutMsg: 'Yes button did not appear' });

    await clickPromptButton('Yes');

    // Wait through multiple poll cycles (2s each)
    await browser.pause(6000);

    const labels = await getPromptButtonLabels();
    expect(labels.length).toBe(0);
  });
});
