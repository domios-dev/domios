/**
 * 13-canvas — reconnecting-banner hysteresis (stabilization pass).
 *
 * Proves the calm-reconnect contract on a REAL plan board (the resilient query
 * must hold real data, else `hasDataRef` is false and `reconnecting` can never
 * flip — a synthetic perf fixture paints a board but leaves the query empty):
 *  1. A healthy board never flashes the banner across sampled refetch kicks
 *     (the "steady 200s but flashing every ~500ms" regression).
 *  2. A SINGLE forced /api/plans failure stays silent (hysteresis); a second
 *     consecutive failure shows the banner; recovery clears it on first success.
 *
 * The web client registers a workbox service worker that proxies /api/* via its
 * own `fetch()`, which bypasses `page.route()`. We open the test context with
 * `serviceWorkers: "block"` so plans fetches stay on the page thread where
 * route interception (and the 503 fulfill below) actually takes effect.
 *
 * Banner testids (all gated by `reconnecting && itemCount > 0`):
 *   canvas- / mission-control- / plan-board- / plan-list-reconnecting.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  closeWorkspaceTabs,
  expandWorkspaceSidebar,
  openSidebarIfNeeded,
} from "../../helpers/canvas-inspect";
import { closeCanvasWebSession, openCanvasWebPage } from "../../helpers/canvas-web-page";
import type { Browser, Page } from "@playwright/test";
import type { ApiClient } from "../../helpers/api-client";

const BANNER_SEL =
  '[data-testid="canvas-reconnecting"], [data-testid="mission-control-reconnecting"], [data-testid="plan-board-reconnecting"], [data-testid="plan-list-reconnecting"]';

const PLANS_GLOB = "**/api/plans**";

interface CreatePlanResponse {
  plan_id: string;
}

async function bannerCount(page: Page): Promise<number> {
  return page.locator(BANNER_SEL).count();
}

// Force the resilient query to re-run without a real bridge event: the hook
// re-fetches on the `codeterm-web-boot-changed` window event. `run(true)`
// resets backoff but PRESERVES the consecutive-failure counter (only a
// success resets it), so two kicks over a severed route = two real failures.
async function kickRefetch(page: Page): Promise<void> {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-web-boot-changed")));
}

async function seedRealPlanAndOpenBoard(
  api: ApiClient,
  webBrowser: Browser,
): Promise<{ page: Page; workspaceId: string; close: () => Promise<void> }> {
  const projectPath = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-reconnecting-banner-")),
  );
  await api.createWorkspace("reconnecting banner gate", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");

  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Reconnecting Banner Synthetic Plan",
    project_path: projectPath,
    owner: { pane_id: "reconnecting-orchestrator", session_id: null, title: "Reconnecting Orchestrator" },
    kinds: "progress:#38bdf8",
  });
  const section = await api.post<{ section_id: string }>(`/plans/${created.plan_id}/sections`, {
    title: "Reconnecting gate section",
    body: "Plan index needs non-empty plan content.",
  });
  await api.post(`/plans/${created.plan_id}/mode`, { mode: "approved" });
  await api.post(`/plans/${created.plan_id}/tasks`, {
    name: "Reconnecting gate task",
    type: "research",
    sections: section.section_id,
    body: "Task body so the real /api/plans query returns data.",
    parent: null,
  });

  await api.focusWorkspace(workspace.id);
  await closeWorkspaceTabs(api, workspace.id);
  await expect
    .poll(async () => {
      const body = await api.get<{ plans: Array<{ file_path?: string; task_count?: number }> }>("/plans");
      const row = body.plans.find((p) => (p.file_path ?? "").includes(projectPath));
      return row != null && (row.task_count ?? 0) > 0;
    })
    .toBe(true);

  // serviceWorkers: "block" — keep /api/plans on the page thread for page.route.
  const session = await openCanvasWebPage(webBrowser, { clearHarness: true, serviceWorkers: "block" });
  const { page } = session;
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openSidebarIfNeeded(page);
  const wsItem = page.locator(`[data-testid="workspace-item"][data-workspace-id="${workspace.id}"]`);
  await expect(wsItem).toBeVisible({ timeout: 30_000 });
  await wsItem.click();
  await expandWorkspaceSidebar(page, workspace.id);

  const entry = page.locator(
    `div:has([data-workspace-id="${workspace.id}"]) [data-testid="workspace-plan-entry"]`,
  );
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await entry.click();
  await expect(page.locator('[data-testid="plan-view-shell"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible({ timeout: 30_000 });

  return { page, workspaceId: workspace.id, close: () => closeCanvasWebSession(session) };
}

test("@golden @canvas-reconnecting healthy board never flashes reconnecting", async ({
  api,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const board = await seedRealPlanAndOpenBoard(api, webBrowser);
  try {
    const { page } = board;
    // Sample repeatedly across ~3s while the board stays healthy; the banner
    // must be absent every time (no lifecycle-driven flashing on refetch).
    for (let i = 0; i < 12; i++) {
      await kickRefetch(page);
      await page.waitForTimeout(250);
      expect(await bannerCount(page), `sample ${i}: banner absent on healthy board`).toBe(0);
    }
  } finally {
    await board.close();
  }
});

test("@golden @canvas-reconnecting one failure is silent, two show, recovery clears", async ({
  api,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const board = await seedRealPlanAndOpenBoard(api, webBrowser);
  try {
    const { page } = board;

    // Sever the event stream so bridge-driven refetches (plan-changed /
    // plans-loaded / resync) can't inject surprise failures between our kicks —
    // only our explicit refetch kicks drive the plans query. SSE loss does not
    // render any plans-reconnecting banner, so this stays a clean confound cut.
    await page.route("**/api/events/stream**", (route) => route.abort("failed"));

    let failPlans = false;
    await page.route(PLANS_GLOB, (route) => {
      if (failPlans) {
        return route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ error: "plans are still loading" }),
        });
      }
      return route.continue();
    });

    // First forced failure: one stale refetch failure stays quiet. Check inside
    // the backoff floor (>=500ms) so the hook's own retry can't reach failure #2
    // before we sample.
    failPlans = true;
    await kickRefetch(page);
    await page.waitForTimeout(250);
    expect(await bannerCount(page), "single failure stays quiet").toBe(0);

    // Second consecutive forced failure: the full-plans counter reaches 2.
    await kickRefetch(page);
    await expect
      .poll(async () => bannerCount(page), {
        message: "two consecutive failures surface the banner",
        timeout: 8_000,
      })
      .toBeGreaterThan(0);

    // Recovery: first success clears the banner.
    failPlans = false;
    await kickRefetch(page);
    await expect
      .poll(async () => bannerCount(page), {
        message: "first success clears the banner",
        timeout: 8_000,
      })
      .toBe(0);

    await page.unroute(PLANS_GLOB);
    await page.unroute("**/api/events/stream**");
  } finally {
    await board.close();
  }
});
