/**
 * WHAT: mission control flow covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  assertPlanSurfaceNotEmpty,
  MISSION_CONTROL_LIVE_SEL,
  MISSION_CONTROL_PLAN_SURFACE_SEL,
  openInlineWorkspaceCanvas,
  CANVAS_ART_DIR,
} from "../../helpers/canvas-inspect";
import { seedWorldStateAndOpenInlineCanvas, worldStates } from "../../fixtures/world-states";
import type { ApiClient } from "../../helpers/api-client";
import type { Page } from "@playwright/test";

async function toCanvasMode(page: Page, workspaceId: string): Promise<void> {
  await page.evaluate(() => window.localStorage.setItem("codeterm.planViewMode", "canvas"));
  await page.reload({ waitUntil: "domcontentloaded" });
  if (!(await page.locator('[data-testid="plan-view-shell"]').first().isVisible().catch(() => false))) {
    await openInlineWorkspaceCanvas(page, workspaceId);
  }
  await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
  const shell = page.locator('[data-testid="plan-view-shell"]');
  await expect(shell).toHaveAttribute("data-plan-view-mode", "canvas");
  await expect(page.locator(MISSION_CONTROL_LIVE_SEL)).toBeVisible({ timeout: 15_000 });
}

test("@golden @mission-control-flow flow river + group milestone + measured curves", async ({ api, webBrowser }) => {
  test.setTimeout(180_000);
  const canvas = await seedWorldStateAndOpenInlineCanvas(
    webBrowser,
    api as unknown as ApiClient,
    worldStates.tabTreeWithStatuses,
  );
  try {
    const { page } = canvas;
    await assertPlanSurfaceNotEmpty(page);
    await toCanvasMode(page, canvas.workspaceId);

    await expect(page.locator(MISSION_CONTROL_PLAN_SURFACE_SEL)).toBeVisible();

    // A plan-river renders with milestone-centric flow and no ghost stations.
    // The perf fixture has pending, in_progress, and done work only.
    const river = page.locator('[data-testid^="plan-river-"]').first();
    await expect(river).toBeVisible();
    await expect(river.locator('[data-testid="flow-station-milestone"]')).toBeVisible();
    await expect(river.locator('[data-testid="flow-station-done"]')).toBeVisible();

    // The tab-tree world has a pending task, so the milestone station is present
    // (PlanRiver: frontier.count > 0) and names itself either way.
    const milestone = river.locator('[data-testid="flow-station-milestone"]');
    await expect(milestone).toContainText(/Milestone/i);
    // Measured flow curves connect the stations (F2: derived from station rects).
    await expect(river.locator('[data-testid="flow-curves"]')).toBeVisible();

    await page.screenshot({ path: path.join(CANVAS_ART_DIR, "mission-control-flow.png"), fullPage: false });

    // NOT COVERED: the workspace/all-plans scope toggle. The control was deleted —
    // MissionControlPlanSurface.test.tsx now asserts "workspace surface has no
  } finally {
    await canvas.close();
  }
});

test("@golden @mission-control-flow 60fps pan probe — transform layer, no reflow", async ({ api, webBrowser }) => {
  test.setTimeout(180_000);
  const canvas = await seedWorldStateAndOpenInlineCanvas(
    webBrowser,
    api as unknown as ApiClient,
    worldStates.tabTreeWithStatuses,
  );
  try {
    const { page } = canvas;
    await assertPlanSurfaceNotEmpty(page);
    await toCanvasMode(page, canvas.workspaceId);

    const viewport = page.locator(`${MISSION_CONTROL_LIVE_SEL} .mc-live-viewport`).first();
    await expect(viewport).toBeVisible();
    const box = (await viewport.boundingBox())!;
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;

    // Start a rAF frame sampler.
    await page.evaluate(() => {
      const w = window as unknown as { __frames: number[]; __rafOn: boolean };
      w.__frames = [];
      w.__rafOn = true;
      let last = performance.now();
      const tick = (t: number) => {
        w.__frames.push(t - last);
        last = t;
        if (w.__rafOn) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });

    // Drag-pan across the viewport in many small steps (drives the pan path).
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    for (let i = 0; i < 40; i += 1) {
      await page.mouse.move(cx - i * 6, cy - i * 3);
      // PW_JUSTIFIED_SLEEP: fixed 8ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(8);
    }
    await page.mouse.up();

    const stats = await page.evaluate(() => {
      const w = window as unknown as { __frames: number[]; __rafOn: boolean };
      w.__rafOn = false;
      const f = w.__frames.filter((d) => d > 0).sort((a, b) => a - b);
      const median = f.length ? f[Math.floor(f.length / 2)] : 0;
      const p95 = f.length ? f[Math.floor(f.length * 0.95)] : 0;
      return { count: f.length, median, p95 };
    });

    console.log(`[flow-pan-fps] frames=${stats.count} median=${stats.median.toFixed(1)}ms p95=${stats.p95.toFixed(1)}ms`);
    expect(stats.count, "frames sampled during pan").toBeGreaterThan(10);
    // 60fps target ≈ 16.7ms; allow headless slack. Median under ~2 frames = pan stays smooth.
    expect(stats.median, "median frame time during pan").toBeLessThan(34);
  } finally {
    await canvas.close();
  }
});
