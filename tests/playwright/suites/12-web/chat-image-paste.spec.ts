/**
 * 12-web / chat-image-paste — pasting an image into the chat composer attaches
 * it (instead of leaking to the terminal). Guards #152.
 *
 * Headless msedge's real/overridden Clipboard API hangs the page, so we dispatch
 * a synthetic paste ClipboardEvent carrying an image File — exercising the
 * handler's image-capture → storeBlob (→ /api/upload) → attach path. The async
 * Clipboard API branch (the WebView2 screenshot case) is covered by live verify.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

const WEB_URL = `${API_BASE}/web/`;

// 1x1 transparent PNG.
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

test("@golden pasting a screenshot into the chat composer attaches it", async ({ api, webBrowser, paneCleanup, spawnAgent }) => {
  test.setTimeout(120_000);
  const ctx = await webBrowser.newContext({ viewport: { width: 375, height: 812 } });
  const page: Page = await ctx.newPage();

  await paneCleanup.closeUnprotectedPanes();
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  const inbox = await paneCleanup.createPane({ title: "pw-imgpaste-inbox" });
  let agentPaneId: string | undefined;
  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-imgpaste-agent",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: "Wait silently.",
    });
    agentPaneId = spawned.pane_id;

    const composer = page.locator('[data-testid="chat-composer"]').filter({ visible: true }).first();
    await composer.waitFor({ timeout: 60_000 });
    await composer.focus();

    // Synthetic paste of an image File → handler captures it, storeBlob →
    // /api/upload → attachment chip.
    await page.evaluate((b64) => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const file = new File([bytes], "screenshot.png", { type: "image/png" });
      const dt = new DataTransfer();
      dt.items.add(file);
      const ev = new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true });
      (document.activeElement ?? document.body).dispatchEvent(ev);
    }, PNG_B64);

    await expect(page.locator('[data-testid="attachment-chip"]').first()).toBeVisible({ timeout: 10_000 });
  } finally {
    if (agentPaneId) paneCleanup.track(agentPaneId);
    await ctx.close();
  }
});
