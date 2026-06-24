/**
 * 13-canvas — worlds-v3 GEOMETRY acceptance criterion (crisp-text-v2 + cloud-left).
 *
 * worlds-v3 moved TASKS into a CLOUD to the RIGHT of the plan, so the old
 * center-zoom-in-place spec (canvas-worlds.spec.ts) can never reach a task card.
 * This spec verifies the REAL worlds-v3 geometry across camera scales:
 *
 *   1. FOOTPRINT FIXED ACROSS SCALES (§22.1/§32.4) — a node's WORLD footprint
 *      (screen box ÷ cameraScale) is constant across zoom; only on-screen px scale.
 *      Verified for a TASK card and a PLAN node.
 *   2. BODY REVEAL — zooming in until a task's appW enters the W2 band reveals
 *      canvas-task-body (the worlds-v3-correct form of the old assertion).
 *   3. CLOUD-LEFT (§35) — active (not-done) task nodes have mean-x LESS than
 *      done/cancelled nodes' mean-x (active pulled plan-side/left, done sunk right).
 *   4. CRISP TEXT (§34) — at deep zoom a W2+ task card carries the crisp wrapper
 *      (.ct-canvas-crisp, transform-origin 0 0 + counter-scale).
 *
 * Skips (does not fail) on an empty board or a board with no task cards.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=full --grep @canvas-worlds-v3
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import {
  canvasScale,
  captureBoard,
  collectBoxes,
  isEmpty,
  openCanvas,
  setCanvasScale,
  zoomBy,
} from "../../helpers/canvas-inspect";

type World = "W0" | "W1" | "W2" | "W3";

// Final rendered screen box of a node (after world transform + crisp zoom/counter-
// scale, which net to ×1). Selected by data-task-id when given, else the first match.
async function screenBox(page: Page, testid: string, taskId?: string): Promise<{ w: number; h: number; x: number } | null> {
  return page.evaluate(({ testid, taskId }) => {
    const sel = taskId ? `[data-testid="${testid}"][data-task-id="${taskId}"]` : `[data-testid="${testid}"]`;
    const el = document.querySelector(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return null;
    return { w: r.width, h: r.height, x: r.x };
  }, { testid, taskId });
}

// World footprint = screen box ÷ cameraScale; invariant across zoom (§22.1).
async function worldFootprint(page: Page, testid: string, taskId?: string): Promise<{ w: number; h: number } | null> {
  const box = await screenBox(page, testid, taskId);
  if (!box) return null;
  const scale = await canvasScale(page);
  if (scale <= 0) return null;
  return { w: box.w / scale, h: box.h / scale };
}

async function visible(page: Page, testid: string): Promise<boolean> {
  return page.evaluate((tid) => {
    const el = document.querySelector(`[data-testid="${tid}"]`) as HTMLElement | null;
    if (!el) return false;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) < 0.05) return false;
    const r = el.getBoundingClientRect();
    return r.width > 2 && r.height > 2;
  }, testid);
}

async function taskDiveSnapshot(page: Page, taskId: string) {
  return page.evaluate((id) => {
    const card = document.querySelector(`[data-testid="canvas-task-card"][data-task-id="${id}"]`) as HTMLElement | null;
    if (!card) return null;
    const cardRect = card.getBoundingClientRect();
    const isVisible = (el: Element | null) => {
      if (!el) return false;
      const html = el as HTMLElement;
      const cs = getComputedStyle(html);
      const r = html.getBoundingClientRect();
      return r.width > 2 && r.height > 2 && cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) >= 0.05;
    };
    const relBox = (testid: string) => {
      const el = card.querySelector(`[data-testid="${testid}"]`) as HTMLElement | null;
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return {
        x: (r.left - cardRect.left) / cardRect.width,
        y: (r.top - cardRect.top) / cardRect.height,
        w: r.width / cardRect.width,
        h: r.height / cardRect.height,
      };
    };
    const title = card.querySelector('[data-testid="canvas-task-name"]') as HTMLElement | null;
    const detailIds = [
      "canvas-task-status",
      "canvas-task-name",
      "canvas-task-counters",
      "canvas-task-meta-slot",
      "canvas-task-body",
      "canvas-task-full-body",
      "canvas-task-steps",
      "canvas-task-step-card",
    ];
    return {
      world: card.getAttribute("data-world") as World | null,
      titleText: (title?.textContent ?? "").trim(),
      fullTitle: title?.getAttribute("data-task-name-full") ?? "",
      boxes: {
        name: relBox("canvas-task-name"),
        meta: relBox("canvas-task-meta-slot"),
        summary: relBox("canvas-task-summary-slot"),
        bodySlot: relBox("canvas-task-body-slot") ?? relBox("canvas-task-body"),
        strides: relBox("canvas-task-strides-slot"),
      },
      visibleDetails: detailIds.filter((testid) => isVisible(card.querySelector(`[data-testid="${testid}"]`))).length,
      overlaps: (() => {
        const ids = ["canvas-task-name", "canvas-task-meta-slot", "canvas-task-summary-slot", "canvas-task-body", "canvas-task-strides-slot"];
        const rects = ids
          .map((testid) => {
            const el = card.querySelector(`[data-testid="${testid}"]`) as HTMLElement | null;
            if (!el || !isVisible(el)) return null;
            const r = el.getBoundingClientRect();
            return { testid, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
          })
          .filter(Boolean) as Array<{ testid: string; left: number; right: number; top: number; bottom: number }>;
        const hits: string[] = [];
        for (let i = 0; i < rects.length; i++) {
          for (let j = i + 1; j < rects.length; j++) {
            const a = rects[i];
            const b = rects[j];
            const x = Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left));
            const y = Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
            if (x * y > 4) hits.push(`${a.testid}/${b.testid}`);
          }
        }
        return hits;
      })(),
    };
  }, taskId);
}

// ≤5% + 2px: the crisp zoom/counter-scale cancels to exactly 1× world size, only
// raster sharpness shifts a few %; sub-pixel rect rounding adds the small floor.
function footprintEqual(a: number, b: number): boolean {
  return Math.abs(a - b) <= Math.max(2, 0.05 * Math.max(a, b));
}

function relClose(a: number, b: number): boolean {
  return Math.abs(a - b) <= 0.035;
}

test("@golden @canvas-worlds-v3 worlds-v3 geometry: footprint fixed, body reveals, cloud-left, crisp", async ({ ui }) => {
  test.setTimeout(120_000);
  const { page } = ui;

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace — seed a plan to verify worlds-v3");
    return;
  }
  // Task cards mount only at a close LOD band (W0 birds-eye renders silhouettes
  // without the full TaskCard); zoom in before collecting so they exist.
  await setCanvasScale(page, 1.6);
  const taskCards = await collectBoxes(page, "canvas-task-card");
  if (taskCards.length === 0) {
    test.skip(true, "plan has no task cards — worlds-v3 cloud geometry needs at least one task");
    return;
  }
  const probeTaskId = taskCards[0].id;
  if (!probeTaskId) {
    test.skip(true, "task card has no data-task-id — worlds-dive needs a stable probe task");
    return;
  }

  // ── 1. FOOTPRINT FIXED ACROSS SCALES (task + plan), §22.1/§32.4. ──
  const scales = [0.7, 1.3, 2.3];
  const taskFp: Array<{ w: number; h: number }> = [];
  const planFp: Array<{ w: number; h: number }> = [];
  for (const s of scales) {
    await setCanvasScale(page, s);
    const tf = await worldFootprint(page, "canvas-task-card", probeTaskId);
    const pf = await worldFootprint(page, "canvas-plan-node");
    if (tf) taskFp.push(tf);
    if (pf) planFp.push(pf);
  }
  await captureBoard(page, "worlds-v3-footprint");
  expect(taskFp.length, "task footprint sampled across scales").toBeGreaterThanOrEqual(2);
  for (let i = 1; i < taskFp.length; i++) {
    expect(footprintEqual(taskFp[i].w, taskFp[0].w), `task world width fixed across zoom (${JSON.stringify(taskFp)})`).toBe(true);
    expect(footprintEqual(taskFp[i].h, taskFp[0].h), `task world height fixed across zoom (${JSON.stringify(taskFp)})`).toBe(true);
  }
  expect(planFp.length, "plan footprint sampled across scales").toBeGreaterThanOrEqual(2);
  for (let i = 1; i < planFp.length; i++) {
    expect(footprintEqual(planFp[i].w, planFp[0].w), `plan world width fixed across zoom (${JSON.stringify(planFp)})`).toBe(true);
    expect(footprintEqual(planFp[i].h, planFp[0].h), `plan world height fixed across zoom (${JSON.stringify(planFp)})`).toBe(true);
  }

  // ── 2. BODY REVEAL — zoom in until a task's appW enters the W2 content band. ──
  // (We are already near max scale 2.3; zoom in further to be safe across footprints.)
  let taskBody = await visible(page, "canvas-task-body");
  for (let i = 0; i < 10 && !taskBody; i++) {
    await zoomBy(page, -160);
    taskBody = await visible(page, "canvas-task-body");
  }
  await captureBoard(page, "worlds-v3-body");
  // §35.3 (zoom-into-task body reveal): the DESCRIPTION box is a fixed slot that
  // reveals at the W2 content band — reachable by zoom alone, content-independent.
  expect(taskBody, "canvas-task-body revealed by zoom into the W2 content band (§35.3)").toBe(true);

  // ── 2b. WORLDS-DIVE: inner boxes stay put while detail count increases. ──
  const diveScales = [0.7, 1, 1.6, 2.4, 3.6, 5.5, 7.5, 10];
  const snapshots: Array<NonNullable<Awaited<ReturnType<typeof taskDiveSnapshot>>> & { scale: number }> = [];
  for (const s of diveScales) {
    await setCanvasScale(page, s);
    const snap = await taskDiveSnapshot(page, probeTaskId);
    if (snap?.world) snapshots.push({ ...snap, scale: s });
  }
  const byWorld = new Map<World, (typeof snapshots)[number]>();
  for (const snap of snapshots) if (!byWorld.has(snap.world!)) byWorld.set(snap.world!, snap);
  expect(byWorld.has("W1"), `sampled W1 task state (${JSON.stringify(snapshots.map((s) => [s.scale, s.world]))})`).toBe(true);
  expect(byWorld.has("W2"), `sampled W2 task state (${JSON.stringify(snapshots.map((s) => [s.scale, s.world]))})`).toBe(true);
  expect(byWorld.has("W3"), `sampled W3 task state (${JSON.stringify(snapshots.map((s) => [s.scale, s.world]))})`).toBe(true);

  for (const snap of snapshots) {
    expect(snap.overlaps, `no visible inner-box overlap at ${snap.world} scale ${snap.scale}`).toEqual([]);
  }

  const baseline = snapshots.find((s) => s.boxes.name && s.boxes.meta && s.boxes.summary && s.boxes.bodySlot && s.boxes.strides);
  expect(baseline, "has a complete inner-slot baseline sample").toBeTruthy();
  if (baseline) {
    for (const snap of snapshots) {
      for (const key of Object.keys(baseline.boxes) as Array<keyof typeof baseline.boxes>) {
        const a = baseline.boxes[key];
        const b = snap.boxes[key];
        if (!a || !b) continue;
        expect(relClose(a.x, b.x), `${key}.x stable at ${snap.world} scale ${snap.scale}`).toBe(true);
        expect(relClose(a.y, b.y), `${key}.y stable at ${snap.world} scale ${snap.scale}`).toBe(true);
      }
    }
  }

  const orderedWorlds: World[] = ["W0", "W1", "W2", "W3"];
  const detailByWorld = orderedWorlds.map((w) => byWorld.get(w)).filter(Boolean) as Array<(typeof snapshots)[number]>;
  for (let i = 1; i < detailByWorld.length; i++) {
    expect(
      detailByWorld[i].visibleDetails,
      `progressive reveal ${detailByWorld[i - 1].world}->${detailByWorld[i].world}`,
    ).toBeGreaterThan(detailByWorld[i - 1].visibleDetails);
  }

  for (const snap of snapshots.filter((s) => s.world !== "W0")) {
    const minReadable = Math.min(8, snap.fullTitle.trim().length);
    expect(snap.titleText.length, `title is legible at ${snap.world}: "${snap.titleText}"`).toBeGreaterThanOrEqual(minReadable);
  }

  // ── 4. CRISP TEXT (§34) — at this deep zoom a W2+ card has the crisp wrapper. ──
  const scaleNow = await canvasScale(page);
  if (scaleNow > 1.05) {
    const crisp = await page.evaluate(() => {
      const wrap = document.querySelector(".ct-canvas-crisp") as HTMLElement | null;
      if (!wrap) return null;
      const cs = getComputedStyle(wrap);
      return { origin: cs.transformOrigin, transform: cs.transform };
    });
    expect(crisp, "crisp wrapper (.ct-canvas-crisp) present at deep zoom (W2+)").not.toBeNull();
    expect(crisp?.origin, `crisp transform-origin pinned to 0 0 (got ${crisp?.origin})`).toMatch(/^0px 0px/);
    expect(crisp?.transform, "crisp wrapper carries a counter-scale transform").not.toBe("none");
  }

  // ── 3. CLOUD-LEFT (§35) — active (not-done) mean-x < done/cancelled mean-x. ──
  // Re-fit to a wide view so both halves of the cloud are in DOM, then bucket by shelf.
  await setCanvasScale(page, 0.7);
  const shelves = await page.$$eval('[data-testid="canvas-task-card"]', (els) =>
    els.map((el) => ({ shelf: el.getAttribute("data-shelf"), x: el.getBoundingClientRect().x })),
  );
  const done = shelves.filter((s) => s.shelf === "done").map((s) => s.x);
  const active = shelves.filter((s) => s.shelf !== "done").map((s) => s.x); // active + normal
  if (done.length > 0 && active.length > 0) {
    const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
    const meanActive = mean(active);
    const meanDone = mean(done);
    expect(meanActive, `active tasks pull LEFT of done (§35): meanActive=${meanActive.toFixed(0)} < meanDone=${meanDone.toFixed(0)}`).toBeLessThan(meanDone);
  } else {
    // Single-status board can't show the left/right split — record, don't fail.
    test.info().annotations.push({ type: "cloud-left", description: `skipped split assertion: active=${active.length} done=${done.length}` });
  }
});
