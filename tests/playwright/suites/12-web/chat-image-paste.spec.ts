/**
 * WHAT: chat image paste covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import { isStubTransportActive } from "../../helpers/stub-gate";

const WEB_URL = `${API_BASE}/`;

// 1x1 transparent PNG.
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

test("@stub pasting a screenshot into the chat composer attaches it", async ({ api, webBrowser, tabCleanup, spawnAgent }) => {
  test.setTimeout(120_000);
  // The composer, not the provider, is under test — spawn through the stub
  // transport so the assertion never depends on provider credit.
  test.skip(
    !(await isStubTransportActive(api)),
    "dev daemon is not running with CODETERM_STUB_AGENT_CMD",
  );
  const ctx = await webBrowser.newContext({ viewport: { width: 375, height: 812 } });
  const page: Page = await ctx.newPage();

  await tabCleanup.closeUnprotectedTabs();
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  const inbox = await tabCleanup.createTab({ title: "pw-imgpaste-inbox" });
  let agentTabId: string | undefined;
  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-imgpaste-agent",
      parent_tab_id: inbox.id,
      report_target_tab_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: "Wait silently.",
    });
    agentTabId = spawned.tab_id;

    const composer = page.locator('[data-testid="chat-composer"]').filter({ visible: true }).first();
    await composer.waitFor({ timeout: 60_000 });
    await composer.focus();

    // /api/upload → attachment chip.
    await page.evaluate((b64) => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const file = new File([bytes], "screenshot.png", { type: "image/png" });
      const dt = new DataTransfer();
      dt.items.add(file);
      const ev = new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true });
      (document.activeElement ?? document.body).dispatchEvent(ev);
    }, PNG_B64);

    const chip = page.locator('[data-testid="attachment-chip"]').first();
    await expect(chip).toBeVisible({ timeout: 10_000 });
    await expect(chip).toContainText(/screenshot\.png|image/i);
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
    await ctx.close();
  }
});
