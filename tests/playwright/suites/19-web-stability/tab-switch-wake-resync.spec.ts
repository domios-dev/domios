/**
 * WHAT: A wake resync cannot overwrite an immediate user tab switch with stale active_tab_id.
 * WHY: Browser foregrounding schedules a delayed mux refresh that may race the next click.
 * ORACLE: After hidden->visible, clicking B shows B active and never bounces back to A.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { setHidden } from "../../helpers/visibility-emulation";

const WEB_URL = `${API_BASE}/`;

type CreatedPane = {
  id: string;
  tab_id: string;
  workspace_id?: string;
};

type MuxState = {
  workspaces: {
    id: string;
    tabs: { id: string }[];
    active_tab_id?: string | null;
  }[];
};

async function newWebPage(browser: Browser): Promise<Page> {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  return ctx.newPage();
}

async function openWeb(page: Page): Promise<void> {
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]', {
    timeout: 30_000,
  });
}

async function createWorkspace(api: ApiClient, name: string): Promise<{ id: string }> {
  const result = await api.post<{ id?: string; workspace_id?: string }>("/workspaces", { name, activate: true });
  return { id: result.id ?? result.workspace_id! };
}

async function createTab(api: ApiClient, workspaceId: string, title: string): Promise<CreatedPane> {
  const tabInfo = await api.post<{ tab_id: string; pane_id: string }>(`/workspaces/${workspaceId}/tabs`, { eager: true });
  await api.post(`/workspaces/${workspaceId}/tabs/${tabInfo.tab_id}/rename`, { title });
  return { id: tabInfo.pane_id, tab_id: tabInfo.tab_id, workspace_id: workspaceId };
}

function tab(page: Page, tabId: string) {
  return page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
}

function activePill(page: Page, tabId: string) {
  return tab(page, tabId).locator(".active-tab-pill");
}

async function isActive(page: Page, tabId: string): Promise<boolean> {
  return activePill(page, tabId).isVisible().catch(() => false);
}

test("@golden wake resync does not revert an immediate tab switch", async ({ api, webBrowser, paneCleanup }) => {
  const ws = await createWorkspace(api, `pw-wake-tab-${Date.now()}`);
  const paneA = await createTab(api, ws.id, "pw-wake-a");
  const paneB = await createTab(api, ws.id, "pw-wake-b");
  paneCleanup.track(paneA.id);
  paneCleanup.track(paneB.id);
  const page = await newWebPage(webBrowser);

  try {
    await openWeb(page);
    await expect(tab(page, paneA.tab_id)).toBeVisible({ timeout: 20_000 });
    await expect(tab(page, paneB.tab_id)).toBeVisible({ timeout: 20_000 });

    await tab(page, paneA.tab_id).click();
    await expect(activePill(page, paneA.tab_id)).toBeVisible({ timeout: 300 });
    await expect.poll(async () => {
      const mux = await api.get<MuxState>("/mux/state");
      return mux.workspaces.find((workspace) => workspace.id === ws.id)?.active_tab_id;
    }, { intervals: [20], timeout: 2_000 }).toBe(paneA.tab_id);

    await page.route(`**/api/workspaces/${ws.id}/tabs/${paneB.tab_id}/focus`, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 400));
      await route.continue();
    });

    await setHidden(page, true);
    await setHidden(page, false);
    await tab(page, paneB.tab_id).click();

    await expect.poll(() => isActive(page, paneB.tab_id), { intervals: [20], timeout: 300 }).toBe(true);

    let revertCount = 0;
    const deadline = Date.now() + 500;
    while (Date.now() < deadline) {
      if (await isActive(page, paneA.tab_id)) {
        revertCount += 1;
      }
      await page.waitForTimeout(20);
    }
    expect(revertCount).toBe(0);
  } finally {
    await page.context().close();
    await api.closeWorkspace(ws.id).catch(() => {});
  }
});
