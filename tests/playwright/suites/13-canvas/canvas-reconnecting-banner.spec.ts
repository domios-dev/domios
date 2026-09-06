/**
 * WHAT: canvas reconnecting banner covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import { expandWorkspaceSidebar, openSidebarIfNeeded } from "../../helpers/canvas-inspect";
import { closeCanvasWebSession, openCanvasWebPage } from "../../helpers/canvas-web-page";
import type { Browser, Page } from "@playwright/test";
import type { ApiClient } from "../../helpers/api-client";
import type { TabCleanup } from "../../helpers/fixtures";

const BANNER_SEL =
  '[data-testid="canvas-reconnecting"], [data-testid="mission-control-reconnecting"], [data-testid="plan-board-reconnecting"], [data-testid="plan-list-reconnecting"]';

const PLANS_GLOB = "**/api/plans**";

interface CreatePlanResponse {
  plan_id: string;
  team_id: string;
  plan: { id: string; title: string };
}

interface TabResponse {
  id: string;
  title?: string;
}

async function bannerCount(page: Page): Promise<number> {
  return page.locator(BANNER_SEL).count();
}

async function kickRefetch(page: Page): Promise<void> {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-web-boot-changed")));
}

// The plan owner must be a live Orchestrator tab: POST /api/plans routes through
// create_team_plan_for_orchestrator, which rejects an unknown id with
async function seedRealPlanAndOpenBoard(
  api: ApiClient,
  tabCleanup: TabCleanup,
  webBrowser: Browser,
): Promise<{ page: Page; workspaceId: string; close: () => Promise<void> }> {
  const projectPath = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-reconnecting-banner-")),
  );
  await api.createWorkspace("reconnecting banner gate", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("created workspace was not listed");

  const ownerTab = await tabCleanup.createTab<TabResponse>({
    workspace_id: workspace.id,
    cwd: projectPath,
    title: "Reconnecting Orchestrator",
  });
  await api.post(`/tabs/${ownerTab.id}/role`, { role: "Orchestrator" });

  const created = await api.post<CreatePlanResponse>("/plans", {
    title: "Reconnecting Banner Synthetic Plan",
    project_path: projectPath,
    owner: { tab_id: ownerTab.id, session_id: null, title: "Reconnecting Orchestrator" },
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

  // A plan's only sidebar home is its Team card; the accessible name is
  // "Open <plan title>", read off the plan the API just created.
  const teamUnit = page.locator(`[data-testid="team-unit"][data-team-id="${created.team_id}"]`);
  await expect(teamUnit).toBeVisible({ timeout: 30_000 });
  const activation = teamUnit.getByTestId("team-card-activation");
  await expect(activation).toHaveAttribute("aria-label", `Open ${created.plan.title}`);
  await activation.click();

  await expect(page.locator('[data-testid="plan-view-shell"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="plan-board-view"]')).toBeVisible({ timeout: 30_000 });

  return { page, workspaceId: workspace.id, close: () => closeCanvasWebSession(session) };
}

test("@golden @canvas-reconnecting healthy board never flashes reconnecting", async ({
  api,
  tabCleanup,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const board = await seedRealPlanAndOpenBoard(api, tabCleanup, webBrowser);
  try {
    const { page } = board;
    // must be absent every time (no lifecycle-driven flashing on refetch).
    for (let i = 0; i < 12; i++) {
      await kickRefetch(page);
      // PW_JUSTIFIED_SLEEP: fixed 250ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(250);
      expect(await bannerCount(page), `sample ${i}: banner absent on healthy board`).toBe(0);
    }
  } finally {
    await board.close();
  }
});

test("@golden @canvas-reconnecting one failure is silent, two show, recovery clears", async ({
  api,
  tabCleanup,
  webBrowser,
}) => {
  test.setTimeout(180_000);
  const board = await seedRealPlanAndOpenBoard(api, tabCleanup, webBrowser);
  try {
    const { page } = board;

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

    failPlans = true;
    await kickRefetch(page);
    // PW_JUSTIFIED_SLEEP: fixed 250ms sampling/settle window; no app event exists for this probe.
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
