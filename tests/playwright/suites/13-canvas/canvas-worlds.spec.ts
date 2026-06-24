/**
 * 13-canvas — §26 WORLDS content-LOD verification. Drives the camera so a node's
 * apparent on-screen width (appW) crosses the W0→W3 bands and asserts the headline
 * fix: SPEC, PLAN, and TASK BODIES become visible at the content band (W2+), the
 * node FOOTPRINT stays fixed across zoom (content adapts, not size), and zooming
 * out hides bodies again (territories). Skips (does not fail) on an empty board.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=full --grep @canvas-worlds
 */
import { expect, test } from "../../helpers/fixtures";
import {
  captureBoard,
  collectBoxes,
  isEmpty,
  openCanvas,
  setCanvasScale,
  zoomBy,
} from "../../helpers/canvas-inspect";

// True when the element exists AND has a non-trivial rendered box (not a 0-size shell).
async function visible(page: import("@playwright/test").Page, testid: string): Promise<boolean> {
  return page.evaluate((tid) => {
    const el = document.querySelector(`[data-testid="${tid}"]`);
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return r.width > 2 && r.height > 2;
  }, testid);
}

async function planNodeCssWidth(page: import("@playwright/test").Page): Promise<number | null> {
  return page.evaluate(() => {
    const el = document.querySelector('[data-testid="canvas-plan-node"]') as HTMLElement | null;
    return el ? el.offsetWidth : null;
  });
}

test("@golden @canvas-worlds bodies reveal at W2 for spec/plan/task; footprint fixed across zoom", async ({ ui }) => {
  test.setTimeout(120_000);
  const { page } = ui;

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace — seed a plan to verify worlds");
    return;
  }

  // Footprint reference: the plan node's CSS width is set from the layout box and
  // must NOT change with zoom (§22.1/§26.4 — content adapts, the frame does not).
  await setCanvasScale(page, 0.9);
  const widthMid = await planNodeCssWidth(page);

  // ── W0 MASS: zoom far out — bodies hidden (board reads as territories). ──
  await zoomBy(page, 240);
  await zoomBy(page, 240);
  await captureBoard(page, "worlds-W0-far");
  expect(await visible(page, "canvas-spec-section-body"), "spec body hidden at far zoom (W0)").toBe(false);
  expect(await visible(page, "canvas-task-body"), "task body hidden at far zoom (W0)").toBe(false);
  const widthFar = await planNodeCssWidth(page);

  // ── W2/W3 CONTENT: zoom in until a node's appW enters the body band. ──
  let specBody = false;
  let planBody = false;
  let taskBody = false;
  for (let i = 0; i < 12 && !(specBody && planBody && taskBody); i++) {
    await zoomBy(page, -150);
    specBody = specBody || (await visible(page, "canvas-spec-section-body"));
    planBody = planBody || (await visible(page, "canvas-plan-section-body"));
    taskBody = taskBody || (await visible(page, "canvas-task-body"));
  }
  await captureBoard(page, "worlds-W2-content");

  // THE bug fix: bodies are reachable by zoom for all three node types.
  expect(specBody, "SPEC section body visible at content band (W2+)").toBe(true);
  expect(planBody, "PLAN section body visible at content band (W2+) — not index-only").toBe(true);
  expect(taskBody, "TASK body visible at content band (W2+)").toBe(true);

  // RichMarkdown emits distinct block elements, never a flattened text blob.
  const blocks = await page.evaluate(() =>
    document.querySelectorAll(
      '[data-testid="canvas-spec-section-body"] p, [data-testid="canvas-spec-section-body"] li, [data-testid="canvas-spec-section-body"] pre, [data-testid="canvas-spec-section-body"] h1, [data-testid="canvas-spec-section-body"] h2',
    ).length,
  );
  expect(blocks, "spec body renders structured markdown blocks").toBeGreaterThan(0);

  // Footprint fixed across all the zoom we just did (≤2px tolerance).
  const widthNear = await planNodeCssWidth(page);
  expect(widthMid, "plan node CSS width measured").not.toBeNull();
  expect(Math.abs((widthNear ?? 0) - (widthMid ?? 0)), "footprint width fixed across zoom").toBeLessThanOrEqual(2);
  expect(Math.abs((widthFar ?? 0) - (widthMid ?? 0)), "footprint width fixed far→mid").toBeLessThanOrEqual(2);

  // No task-card overlap at the content band (reserve-at-deepest packing).
  const tasks = await collectBoxes(page, "canvas-task-card");
  for (let a = 0; a < tasks.length; a++) {
    for (let b = a + 1; b < tasks.length; b++) {
      const x = Math.max(0, Math.min(tasks[a].box.x + tasks[a].box.width, tasks[b].box.x + tasks[b].box.width) - Math.max(tasks[a].box.x, tasks[b].box.x));
      const y = Math.max(0, Math.min(tasks[a].box.y + tasks[a].box.height, tasks[b].box.y + tasks[b].box.height) - Math.max(tasks[a].box.y, tasks[b].box.y));
      expect(x * y, `task cards ${tasks[a].id} / ${tasks[b].id} do not overlap`).toBeLessThanOrEqual(4);
    }
  }
});
