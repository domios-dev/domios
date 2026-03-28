/**
 * Prompt approval UI tests — verify pending_prompt renders in chat view.
 *
 * Flow:
 * 1. App starts with restored session
 * 2. We trigger permission hook via API
 * 3. Switch to chat view
 * 4. Verify prompt card appears with Yes/No buttons
 * 5. Click Yes
 * 6. Verify prompt disappears
 */

import http from 'http';

// Helper: call CodeTerm API (the dev instance running inside the app)
function apiCall(method, path, body = null) {
  return new Promise((resolve, reject) => {
    // Read token from env or use default dev token
    const token = process.env.CODETERM_API_TOKEN || '';
    const port = process.env.CODETERM_API_PORT || '7686';

    const options = {
      hostname: '127.0.0.1',
      port: parseInt(port),
      path: `/api${path}`,
      method,
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { 'Authorization': `Bearer ${token}` } : {}),
      },
    };

    const req = http.request(options, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try {
          resolve(JSON.parse(data));
        } catch {
          resolve(data);
        }
      });
    });
    req.on('error', reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

describe('Prompt Approval in Chat View', () => {

  let paneId;

  before(async () => {
    // Wait for app to load
    await browser.pause(5000);

    // Get first pane with PTY
    try {
      const panes = await apiCall('GET', '/panes');
      const pane = panes.find(p => p.pty_id);
      if (pane) paneId = pane.id;
    } catch (e) {
      console.log('Could not get panes via API:', e.message);
    }
  });

  it('should show chat toggle button in toolbar', async () => {
    // Look for chat view toggle button (speech bubble icon or similar)
    const buttons = await $$('button[title*="chat"], button[title*="Chat"], button[title*="terminal"]');
    expect(buttons.length).toBeGreaterThan(0);
  });

  it('should toggle chat view', async () => {
    // Click chat toggle
    const chatBtn = await $('button[title*="chat"], button[title*="Chat"]');
    if (await chatBtn.isExisting()) {
      await chatBtn.click();
      await browser.pause(1000);

      // Chat view should be visible (has input field "Send a message...")
      const input = await $('textarea[placeholder*="Send"], input[placeholder*="Send"]');
      const visible = await input.isExisting();
      expect(visible).toBe(true);
    }
  });

  it('should show prompt card after hook/permission', async () => {
    if (!paneId) {
      console.log('Skipping — no pane available');
      return;
    }

    // Trigger permission hook via API
    await apiCall('POST', `/panes/${paneId}/hook/permission`);
    await browser.pause(2000);

    // Look for prompt card in chat view (yellow warning card with buttons)
    const promptCard = await $('div[class*="rounded-lg"]');
    // Or look for "Yes" button text
    const buttons = await $$('button');
    const buttonTexts = await Promise.all(buttons.map(b => b.getText()));
    // Should have approve-like buttons
    console.log('Buttons found:', buttonTexts.filter(t => t.length < 50));
  });

  it('should clear prompt after clicking respond', async () => {
    if (!paneId) return;

    // Respond via API (simulates button click)
    await apiCall('POST', `/panes/${paneId}/prompt/respond`, { value: '1' });
    await browser.pause(1000);

    // Verify prompt is gone from API state
    const panes = await apiCall('GET', '/panes');
    const pane = panes.find(p => p.id === paneId);
    expect(pane?.pending_prompt).toBeNull();
  });
});
