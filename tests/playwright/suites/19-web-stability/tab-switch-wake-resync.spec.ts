/**
 * WHAT: A wake resync cannot overwrite an immediate user tab switch with stale active_tab_id.
 * WHY: Browser foregrounding schedules a delayed mux refresh that may race the next click.
 * ORACLE: After hidden->visible, clicking B shows B active and never bounces back to A.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { setHidden } from "../../helpers/visibility-emulation";

const WEB_URL = `${API_BASE}/`;

type CreatedTab = {
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

async function createTab(api: ApiClient, workspaceId: string, title: string): Promise<CreatedTab> {
  const tabInfo = await api.post<{ tab_id: string; tab_id: string }>(`/workspaces/${workspaceId}/tabs`, { eager: true });
  await api.post(`/workspaces/${workspaceId}/tabs/${tabInfo.tab_id}/rename`, { title });
  return { id: tabInfo.tab_id, tab_id: tabInfo.tab_id, workspace_id: workspaceId };
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

test("@golden wake resync does not revert an immediate tab switch", async ({ api, webBrowser, tabCleanup }) => {
  const ws = await createWorkspace(api, `pw-wake-tab-${Date.now()}`);
  const tabA = await createTab(api, ws.id, "pw-wake-a");
  const tabB = await createTab(api, ws.id, "pw-wake-b");
  tabCleanup.track(tabA.id);
  tabCleanup.track(tabB.id);
  const page = await newWebPage(webBrowser);

  try {
    await openWeb(page);
    await expect(tab(page, tabA.tab_id)).toBeVisible({ timeout: 20_000 });
    await expect(tab(page, tabB.tab_id)).toBeVisible({ timeout: 20_000 });

    await tab(page, tabA.tab_id).click();
    await expect(activePill(page, tabA.tab_id)).toBeVisible({ timeout: 300 });
    await expect.poll(async () => {
      const mux = await api.get<MuxState>("/mux/state");
      return mux.workspaces.find((workspace) => workspace.id === ws.id)?.active_tab_id;
    }, { intervals: [20], timeout: 2_000 }).toBe(tabA.tab_id);

    await page.route(`**/api/workspaces/${ws.id}/tabs/${tabB.tab_id}/focus`, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 400));
      await route.continue();
    });

    await setHidden(page, true);
    await setHidden(page, false);
    await tab(page, tabB.tab_id).click();

    await expect.poll(() => isActive(page, tabB.tab_id), { intervals: [20], timeout: 300 }).toBe(true);

    const deadline = Date.now() + 500;
    let sawRevert = false;
    await expect.poll(async () => {
      if (await isActive(page, tabA.tab_id)) {
        sawRevert = true;
      }
      if (sawRevert) {
        return "reverted";
      }
      return Date.now() >= deadline ? "stable" : "pending";
    }, { intervals: [20], timeout: 1_000 }).toBe("stable");
  } finally {
    await page.context().close();
    await api.closeWorkspace(ws.id).catch(() => {});
  }
});
