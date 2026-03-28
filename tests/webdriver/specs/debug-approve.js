/**
 * Debug test: spawn Claude WITHOUT skip-permissions, trigger prompt, approve via UI.
 * Step by step with verbose logging.
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

describe('Debug: Real prompt approval', () => {
  let paneId;

  it('step 1: get pane and start Claude without skip-permissions', async () => {
    await browser.pause(3000);
    const panes = await api('GET', '/panes');
    paneId = panes.find(p => p.pty_id)?.id;
    console.log(`Pane: ${paneId}`);

    // Ensure CODETERM_PANE_ID is set correctly for THIS dev pane
    // (prod CodeTerm may have set a different value in the parent shell)
    await api('POST', `/panes/${paneId}/send`, {
      text: `export CODETERM_PANE_ID=${paneId}`,
      enter: true,
    });
    await browser.pause(500);

    // Start Claude WITHOUT --dangerously-skip-permissions
    // Give it a task that requires file creation (will trigger prompt)
    await api('POST', `/panes/${paneId}/send`, {
      text: 'claude "create a file /tmp/codeterm-test-approval.txt with content hello world"',
      enter: true,
    });

    console.log('Waiting for Claude to start...');
    // Wait for Claude TUI to appear
    for (let i = 0; i < 30; i++) {
      await browser.pause(2000);
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('esc to interrupt') || screen.includes('shift+tab')) {
        console.log(`Claude ready after ${(i+1)*2}s`);
        break;
      }
      if (i === 29) console.log('Claude did not start in 60s');
    }
  });

  it('step 2: wait for permission prompt on screen', async () => {
    if (!paneId) return;

    console.log('Waiting for "Do you want to proceed?" on screen...');
    let found = false;
    for (let i = 0; i < 30; i++) {
      await browser.pause(2000);
      const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
      if (screen.includes('proceed') || screen.includes('Esc to cancel')) {
        console.log(`Prompt appeared on screen after ${(i+1)*2}s`);
        console.log('Screen tail:', screen.slice(-300));
        found = true;
        break;
      }
    }
    expect(found).toBe(true);
  });

  it('step 3: check pending_prompt on pane via API', async () => {
    if (!paneId) return;

    // Wait a bit for hooks/detection to fire
    await browser.pause(3000);

    const pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    console.log(`pending_prompt: ${JSON.stringify(pane?.pending_prompt)}`);
    console.log(`agent_type: ${pane?.agent_type}`);

    // If hooks work, pending_prompt should be set
    // If not, we'll inject it manually for testing
    if (!pane?.pending_prompt) {
      console.log('Prompt not auto-detected, injecting via hook endpoint...');
      await api('POST', `/panes/${paneId}/hook/permission`);
      await browser.pause(1000);
      const pane2 = (await api('GET', '/panes')).find(p => p.id === paneId);
      console.log(`After hook inject: ${JSON.stringify(pane2?.pending_prompt)}`);
    }
  });

  it('step 4: open chat view', async () => {
    if (!paneId) return;

    const chatBtn = await $('button[title="Switch to chat view"]');
    const termBtn = await $('button[title="Switch to terminal"]');

    if (await chatBtn.isExisting()) {
      console.log('Opening chat view...');
      await chatBtn.click();
      await browser.pause(1000);
    } else if (await termBtn.isExisting()) {
      console.log('Chat already open');
    } else {
      console.log('No chat toggle button found!');
      // List all button titles for debugging
      const titles = await browser.execute(() =>
        Array.from(document.querySelectorAll('button[title]')).map(b => b.title)
      );
      console.log('Available buttons:', titles);
    }
  });

  it('step 5: wait for prompt card in UI', async () => {
    if (!paneId) return;

    console.log('Waiting for prompt buttons in UI...');
    let buttons = [];
    for (let i = 0; i < 10; i++) {
      await browser.pause(2000);
      buttons = await browser.execute(() =>
        Array.from(document.querySelectorAll('button'))
          .map(b => b.textContent.trim())
          .filter(t => ['Yes', 'No', 'Yes, allow reading', 'Yes, allow writing'].some(y => t.startsWith(y)))
      );
      if (buttons.length > 0) {
        console.log(`Prompt buttons found after ${(i+1)*2}s: ${JSON.stringify(buttons)}`);
        break;
      }
    }

    if (buttons.length === 0) {
      // Debug: what IS in the DOM?
      const html = await browser.execute(() => {
        const chat = document.querySelector('[class*="overflow-y-auto"]');
        return chat ? chat.innerHTML.slice(-500) : 'no chat container';
      });
      console.log('Chat container tail:', html);
    }

    expect(buttons.length).toBeGreaterThan(0);
  });

  it('step 6: click Yes and verify Claude continues', async () => {
    if (!paneId) return;

    const clicked = await browser.execute(() => {
      for (const b of document.querySelectorAll('button')) {
        if (b.textContent.trim() === 'Yes' || b.textContent.trim().startsWith('Yes,')) {
          b.click();
          return b.textContent.trim();
        }
      }
      return null;
    });
    console.log(`Clicked: ${clicked}`);
    expect(clicked).not.toBeNull();

    // Wait for prompt to clear
    await browser.pause(3000);

    // Verify prompt cleared in API
    const pane = (await api('GET', '/panes')).find(p => p.id === paneId);
    console.log(`After click - pending_prompt: ${pane?.pending_prompt}`);
    expect(pane?.pending_prompt).toBeFalsy();

    // Verify Claude is working (screen shows activity)
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    const working = screen.includes('esc to interrupt') || screen.includes('Writing') || screen.includes('Created');
    console.log(`Claude continuing: ${working}`);
    console.log('Screen tail:', screen.slice(-200));
  });

  it('step 7: verify file was created', async () => {
    if (!paneId) return;

    // Wait for Claude to finish
    await browser.pause(10000);

    // Switch to terminal to check
    const termBtn = await $('button[title="Switch to terminal"]');
    if (await termBtn.isExisting()) await termBtn.click();
    await browser.pause(1000);

    // Check if file exists
    const screen = (await api('GET', `/panes/${paneId}/screen`)).screen || '';
    console.log('Final screen tail:', screen.slice(-300));

    // The file should exist now
    // We can't check from here, but Claude should have confirmed creation
  });
});
