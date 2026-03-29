/**
 * Workspace & tab management UI tests.
 * Uses API for setup, WebDriver for UI verification.
 * All stable — no Claude dependency.
 */

import { api, clickButton, countTabs, getWorkspaceId, openChatView, closeChatView, setAgentType, injectPrompt, clearPrompt, waitForPromptButtons, pageContains } from '../helpers.js';

describe('Tab Management', () => {
  let initialTabs;

  before(async () => {
    await browser.pause(3000);
    initialTabs = await countTabs();
  });

  it('should create new tab via sidebar button', async () => {
    const before = await countTabs();
    await clickButton('New tab in this workspace');
    await browser.waitUntil(async () => (await countTabs()) > before, {
      timeout: 5000, timeoutMsg: 'Tab count did not increase',
    });
    const after = await countTabs();
    expect(after).toBe(before + 1);
  });

  it('should close a tab via close button', async () => {
    const before = await countTabs();
    // Click last close tab button
    await browser.execute(() => {
      const btns = document.querySelectorAll('button[title="Close tab"]');
      btns[btns.length - 1]?.click();
    });
    await browser.waitUntil(async () => (await countTabs()) < before, {
      timeout: 5000, timeoutMsg: 'Tab count did not decrease',
    });
  });
});

describe('Settings Panel', () => {
  it('should open settings', async () => {
    await clickButton('Settings (Ctrl+,)');
    await browser.waitUntil(async () => await pageContains('Appearance'), {
      timeout: 5000, timeoutMsg: 'Settings did not open',
    });
  });

  it('should show all setting tabs', async () => {
    const tabs = ['Appearance', 'Terminal', 'Keybindings', 'Notifications'];
    for (const tab of tabs) {
      expect(await pageContains(tab)).toBe(true);
    }
  });

  it('should switch between tabs', async () => {
    await browser.execute(() => {
      for (const b of document.querySelectorAll('button')) {
        if (b.textContent.trim() === 'Terminal') { b.click(); break; }
      }
    });
    await browser.pause(500);
    // Check terminal-specific settings appear (Scrollback, Cursor, etc.)
    const hasTerminalContent = await pageContains('Scrollback') || await pageContains('cursor') || await pageContains('Copy on');
    expect(hasTerminalContent).toBe(true);
  });

  it('should close settings', async () => {
    // Click backdrop or X button
    await browser.execute(() => {
      // Find the modal backdrop (fixed overlay behind settings)
      const backdrop = document.querySelector('div[class*="fixed"][class*="inset-0"][class*="z-"]');
      if (backdrop) backdrop.click();
    });
    await browser.pause(500);
    // Settings should be gone (no Appearance tab visible)
    // If backdrop didn't work, try X button
    const stillOpen = await pageContains('Cursor Style');
    if (stillOpen) {
      await browser.execute(() => {
        for (const b of document.querySelectorAll('button')) {
          if (b.title === 'Close' || b.textContent.trim() === '×') { b.click(); break; }
        }
      });
    }
  });
});

describe('Command Palette', () => {
  it('should open with Cmd+K', async () => {
    await browser.execute(() => {
      document.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'k', code: 'KeyK', metaKey: true, bubbles: true,
      }));
    });
    await browser.waitUntil(async () => {
      const input = await $('input[placeholder*="command"]');
      return await input.isExisting();
    }, { timeout: 5000, timeoutMsg: 'Command palette did not open' });
  });

  it('should show commands list', async () => {
    expect(await pageContains('New terminal tab')).toBe(true);
  });

  it('should close on backdrop click', async () => {
    await browser.execute(() => {
      const backdrops = document.querySelectorAll('div[class*="fixed"][class*="inset-0"]');
      for (const b of backdrops) { b.click(); }
    });
    await browser.pause(500);
  });
});

describe('Chat View Toggle', () => {
  let paneId;

  before(async () => {
    const pane = await (async () => {
      const panes = await api('GET', '/panes');
      return panes.find(p => p.pty_id);
    })();
    paneId = pane?.id;
    if (paneId) {
      await setAgentType(paneId);
      await browser.pause(1000);
    }
  });

  it('should open chat view', async () => {
    const opened = await openChatView();
    expect(opened).toBe(true);
    const ta = await $('textarea[placeholder*="Send"]');
    expect(await ta.isExisting()).toBe(true);
  });

  it('should have chat input ready', async () => {
    const ta = await $('textarea[placeholder*="Send"]');
    expect(await ta.isExisting()).toBe(true);
  });

  it('should close chat view', async () => {
    await closeChatView();
    await browser.pause(500);
  });

  after(async () => {
    if (paneId) await clearPrompt(paneId);
  });
});
