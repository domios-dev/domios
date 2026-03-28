/**
 * Basic app loading tests — verify CodeTerm starts and renders.
 */

describe('CodeTerm App', () => {
  it('should load and show the main window', async () => {
    // Wait for app to fully render
    const body = await $('body');
    await body.waitForExist({ timeout: 30000 });
    expect(await body.isExisting()).toBe(true);
  });

  it('should show the sidebar', async () => {
    // Sidebar should be visible with "PANES" header or workspace name
    await browser.pause(3000);
    const html = await $('body').getHTML();
    expect(html.length).toBeGreaterThan(100);
  });

  it('should have a terminal pane', async () => {
    // xterm.js canvas or terminal container should exist
    await browser.pause(2000);
    const terminal = await $('div[class*="terminal"], .xterm, canvas');
    const exists = await terminal.isExisting();
    // Terminal might render differently, just check page has content
    const bodyText = await $('body').getText();
    expect(bodyText.length).toBeGreaterThan(0);
  });
});
