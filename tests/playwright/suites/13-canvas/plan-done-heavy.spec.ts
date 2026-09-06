/**
 * WHAT: plan done heavy covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import path from "node:path";
import fs from "node:fs";
import { expect, test } from "../../helpers/fixtures";
import {
  assertPlanSurfaceNotEmpty,
  CANVAS_ART_DIR,
  seedPerfFixtureAndOpenInlineCanvas,
} from "../../helpers/canvas-inspect";
import type { ApiClient } from "../../helpers/api-client";
import { pollUntil } from "../../helpers/poll";
import { buildDoneHeavyPerfFixturePlans } from "../../../../src/components/Canvas/perfFixture";
import { DONE_COLUMN_COLLAPSED_LIMIT } from "../../../../src/components/Canvas/planView/boardModel";
import { TaskStatus } from "../../../../src/components/Canvas/types";

const DONE_HEAVY_KEY = "canvas.perfFixtureProfile";

// Read the oracle off the fixture that owns it, never a number retyped here.
const DONE_HEAVY_PLAN = buildDoneHeavyPerfFixturePlans()[0];
const DONE_TASK_COUNT = DONE_HEAVY_PLAN.tasks.filter((t) => t.status === TaskStatus.Done).length;

async function screenshot(page: import("@playwright/test").Page, name: string): Promise<string> {
  fs.mkdirSync(CANVAS_ART_DIR, { recursive: true });
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}


test(
  "@golden @plan-done-heavy board done column is collapsed by default with show-all toggle",
  async ({ api, webBrowser }) => {
    test.setTimeout(150_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(
      webBrowser,
      api as unknown as ApiClient,
      /* taskCount= */ 1,
    );
    try {
      const { page } = canvas;

      await page.evaluate((key) => window.localStorage.setItem(key, "done-heavy"), DONE_HEAVY_KEY);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
      await assertPlanSurfaceNotEmpty(page);

      const shell = page.locator('[data-testid="plan-view-shell"]');
      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await modeSwitch.getByRole("tab", { name: /board/i }).click();
      await expect(shell).toHaveAttribute("data-plan-view-mode", "board");

      const boardView = page.locator('[data-testid="plan-board-view"]');
      await expect(boardView).toBeVisible({ timeout: 20_000 });

      const doneColumn = boardView.locator('[data-testid="plan-board-column"][data-column="done"]');

      // The fixture is deterministic: DONE_TASK_COUNT done tasks, well over the
      // collapse limit, so the count and both toggle labels are exact.
      await expect(doneColumn.locator('[data-testid="plan-board-column-count"]')).toHaveText(
        String(DONE_TASK_COUNT),
      );
      expect(DONE_TASK_COUNT, "fixture must exceed the collapse limit for this test to mean anything")
        .toBeGreaterThan(DONE_COLUMN_COLLAPSED_LIMIT);
      await expect(doneColumn.locator('[data-testid="plan-board-task-card"]')).toHaveCount(
        DONE_COLUMN_COLLAPSED_LIMIT,
      );

      const showAllBtn = doneColumn.locator('[data-testid="plan-board-done-toggle"]').first();
      await expect(showAllBtn).toHaveText(`Show all ${DONE_TASK_COUNT}`);

      await screenshot(page, "plan-view-board-done-heavy");

      await showAllBtn.click();
      await expect(doneColumn.locator('[data-testid="plan-board-done-toggle"]').first()).toHaveText(
        `Show recent ${DONE_COLUMN_COLLAPSED_LIMIT}`,
      );
      await expect(doneColumn.locator('[data-testid="plan-board-task-card"]')).toHaveCount(DONE_TASK_COUNT);
      await screenshot(page, "plan-view-board-done-heavy-expanded");
    } finally {
      await canvas.close();
    }
  },
);


test(
  "@golden @plan-done-heavy list view done sections show rollup rows",
  async ({ api, webBrowser }) => {
    test.setTimeout(150_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(
      webBrowser,
      api as unknown as ApiClient,
      /* taskCount= */ 1,
    );
    try {
      const { page } = canvas;

      await page.evaluate((key) => window.localStorage.setItem(key, "done-heavy"), DONE_HEAVY_KEY);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
      await assertPlanSurfaceNotEmpty(page);

      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await modeSwitch.getByRole("tab", { name: /list/i }).click();
      await expect(page.locator('[data-testid="plan-view-shell"]')).toHaveAttribute("data-plan-view-mode", "list");

      const listView = page.locator('[data-testid="plan-list-view"]');
      await expect(listView).toBeVisible({ timeout: 20_000 });
      await expect(page.locator('[data-testid="plan-list-scroll"]')).toBeVisible();

      await screenshot(page, "plan-view-list-done-heavy");

      // The list groups by status, and the all-done "Done" group starts collapsed
      // (defaultSectionExpanded → !allTasksDone), so exactly one rollup row shows
      const rollups = listView.locator('[data-testid="plan-list-done-rollup"]');
      await expect(rollups).toHaveCount(1);
      await expect(rollups.first()).toHaveText(`${DONE_TASK_COUNT} done`);
      await screenshot(page, "plan-view-list-done-heavy-rollup");
    } finally {
      await canvas.close();
    }
  },
);


test(
  "@golden @plan-done-heavy canvas done shelf shows compressed count under done-heavy fixture",
  async ({ api, webBrowser }) => {
    test.setTimeout(150_000);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(
      webBrowser,
      api as unknown as ApiClient,
      /* taskCount= */ 1,
    );
    try {
      const { page } = canvas;

      await page.evaluate((key) => window.localStorage.setItem(key, "done-heavy"), DONE_HEAVY_KEY);
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
      await assertPlanSurfaceNotEmpty(page);

      const modeSwitch = page.locator('[data-testid="plan-view-mode-switch"]');
      await modeSwitch.getByRole("tab", { name: /canvas/i }).click();
      await expect(page.locator('[data-testid="plan-view-shell"]')).toHaveAttribute("data-plan-view-mode", "canvas");

      const missionControl = page.locator('[data-testid="mission-control-live"]');
      await expect(missionControl).toBeVisible({ timeout: 20_000 });

      const doneStation = missionControl.locator('[data-testid="flow-station-done"]').first();
      await pollUntil(
        async () => (await doneStation.isVisible().catch(() => false)),
        15_000,
        250,
        "mission-control done station visible",
      );
      await screenshot(page, "plan-view-canvas-done-heavy");

      // Every non-cancelled done task becomes a done_shelf station (no cap in
      // buildMissionControlModel), so the shelf count equals the fixture's.
      await expect(doneStation.locator(".mc-flow-count")).toHaveText(String(DONE_TASK_COUNT));
      await screenshot(page, "plan-view-canvas-done-heavy-shelf");
    } finally {
      await canvas.close();
    }
  },
);
