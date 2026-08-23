/**
 * WHAT: visual-regression protects the key plan surfaces with screenshot goldens.
 * WHY: These baselines replace manual visual review for the board/list/canvas/sidebar/detail surfaces.
 * ORACLE: Seeded fixtures, fixed viewport, web-mode Chromium, and tight screenshot diffs catch layout drift.
 */

// NOT COVERED on Windows: every baseline in visual-regression.spec.ts-snapshots is
// `<name>-golden-darwin.png`, and Playwright resolves `<name>-<project>-<platform>.png`.
// A win32 runner therefore has no baseline: the run fails with "snapshot doesn't
// exist", or silently writes one under --update-snapshots, which asserts nothing.
// A win32 baseline can only come from running the browser and reviewing the images
// by eye (`pnpm test:pw:visual:regen`); it must never be authored blind.
import type { Locator } from "@playwright/test";
import { expect, test, type Page } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { assertPlanSurfaceNotEmpty, openSidebarIfNeeded, setCanvasScale, waitForVisiblePlanCanvas } from "../../helpers/canvas-inspect";
import { seedWorldStateAndOpenInlineCanvas, worldStates, type SeededCanvasWorld } from "../../fixtures/world-states";

const VIEWPORT = { width: 1280, height: 800 };
const DIFF = { maxDiffPixels: 180 };

async function prepareWorld(api: ApiClient, webBrowser: Parameters<typeof seedWorldStateAndOpenInlineCanvas>[0]) {
  const canvas = await seedWorldStateAndOpenInlineCanvas(webBrowser, api, worldStates.planWithKnownTasks);
  await assertPlanSurfaceNotEmpty(canvas.page);
  await canvas.page.setViewportSize(VIEWPORT);
  await stabilize(canvas.page);
  return canvas;
}

async function stabilize(page: Page): Promise<void> {
  await page.addStyleTag({
    content: `
      *, *::before, *::after {
        animation-duration: 0s !important;
        animation-delay: 0s !important;
        transition-duration: 0s !important;
        transition-delay: 0s !important;
        caret-color: transparent !important;
      }
    `,
  });
  await page.evaluate(() => document.fonts?.ready).catch(() => {});
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}

async function isolateSidebarGolden(page: Page): Promise<void> {
  await page.addStyleTag({
    content: `
      [data-testid="workspace-item"][data-workspace-name^="pw-"] {
        display: none !important;
      }

      div:has(> [data-testid="workspace-item"][data-workspace-name^="pw-"]) {
        display: none !important;
      }
    `,
  });

  await page.locator('[data-testid="workspace-item"][data-workspace-name="General"]').evaluate((item) => {
    const count = item.querySelector('.text-\\[9px\\], .text-\\[10px\\], .text-\\[11px\\], .text-xs');
    if (count) {
      count.textContent = "1";
    }
  });
}

// Relative-age chips ("688mo", "20642d") are wall-clock derived; mask them so
// the golden asserts layout/content without rotting daily.
async function expectGolden(locator: Locator, name: string, mask?: Locator[]): Promise<void> {
  await expect(locator).toHaveScreenshot(`${name}.png`, {
    ...DIFF,
    animations: "disabled",
    ...(mask ? { mask } : {}),
  });
}

async function switchMode(page: Page, mode: "board" | "list" | "canvas"): Promise<void> {
  const shell = page.locator('[data-testid="plan-view-shell"]').first();
  await expect(shell).toBeVisible({ timeout: 30_000 });
  await shell.locator('[data-testid="plan-view-mode-switch"]').getByRole("tab", { name: new RegExp(mode, "i") }).click();
  await expect(shell).toHaveAttribute("data-plan-view-mode", mode);
  await stabilize(page);
}

async function close(canvas: SeededCanvasWorld): Promise<void> {
  await canvas.close().catch(() => {});
}

test("@golden @visual-regression board seeded plan matches golden", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await prepareWorld(api as ApiClient, webBrowser);
  try {
    const { page } = canvas;
    await switchMode(page, "board");
    await expect(page.locator('[data-testid="plan-board-task-card"]').first()).toBeVisible({ timeout: 20_000 });
    await expectGolden(page.locator('[data-testid="plan-view-shell"]').first(), "plan-board-seeded", [
      page.locator('[data-testid="plan-board-task-age"]'),
    ]);
  } finally {
    await close(canvas);
  }
});

test("@golden @visual-regression list seeded plan matches golden", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await prepareWorld(api as ApiClient, webBrowser);
  try {
    const { page } = canvas;
    await switchMode(page, "list");
    await expect(page.locator('[data-testid="plan-list-task"]').first()).toBeVisible({ timeout: 20_000 });
    await expectGolden(page.locator('[data-testid="plan-list-view"]').first(), "plan-list-seeded", [
      page.locator('[data-testid="plan-list-age"]'),
    ]);
  } finally {
    await close(canvas);
  }
});

test("@golden @visual-regression canvas fit zoom matches golden", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await prepareWorld(api as ApiClient, webBrowser);
  try {
    const { page } = canvas;
    await switchMode(page, "canvas");
    await waitForVisiblePlanCanvas(page);
    await expect(page.locator('[data-testid="mission-control-live"]').first()).toBeVisible({ timeout: 20_000 });
    await setCanvasScale(page, 0.78);
    await stabilize(page);
    await expectGolden(page.locator('[data-testid="mission-control-live"]').first(), "plan-canvas-fit-zoom");
  } finally {
    await close(canvas);
  }
});

test("@golden @visual-regression sidebar seeded pair tree matches golden", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await seedWorldStateAndOpenInlineCanvas(webBrowser, api as ApiClient, worldStates.tabTreeWithStatuses);
  try {
    const { page } = canvas;
    await assertPlanSurfaceNotEmpty(page);
    await page.setViewportSize(VIEWPORT);
    await openSidebarIfNeeded(page);
    await expect(page.locator('[data-testid="workspace-item"]').first()).toBeVisible({ timeout: 20_000 });
    await isolateSidebarGolden(page);
    await stabilize(page);
    await expectGolden(page.locator('[data-testid="sidebar"], aside, nav').first(), "sidebar-seeded-pair-tree");
  } finally {
    await close(canvas);
  }
});

test("@golden @visual-regression task detail matches golden", async ({ api, webBrowser }) => {
  test.setTimeout(120_000);
  const canvas = await prepareWorld(api as ApiClient, webBrowser);
  try {
    const { page } = canvas;
    await switchMode(page, "board");
    const firstCard = page.locator('[data-testid="plan-board-task-card"]').first();
    await expect(firstCard).toBeVisible({ timeout: 20_000 });
    await firstCard.click();
    const panel = page.locator('[data-testid="plan-detail-panel"]').first();
    await expect(panel).toBeVisible({ timeout: 10_000 });
    await stabilize(page);
    await expectGolden(panel, "task-detail-seeded");
  } finally {
    await close(canvas);
  }
});
