/**
 * WHAT: canvas worlds v3 covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import {
  canvasScale,
  captureBoard,
  focusTaskById,
  isEmpty,
  openCanvas,
  pickProbeTaskId,
  setCanvasScale,
  setCanvasScaleOnTask,
  zoomTowardTask,
  waitForCanvasSettled,
} from "../../helpers/canvas-inspect";

type World = "W0" | "W1" | "W2" | "W3";
const SUBSTRATE_FLAG_KEY = "canvas.substrateV2";

// Final rendered screen box of a node. Selected by data-task-id when given, else the first match.
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

async function worldFootprint(page: Page, testid: string, taskId?: string): Promise<{ w: number; h: number } | null> {
  const box = await screenBox(page, testid, taskId);
  if (!box) return null;
  const scale = await canvasScale(page);
  if (scale <= 0) return null;
  return { w: box.w / scale, h: box.h / scale };
}

function footprintEqual(a: number, b: number): boolean {
  return Math.abs(a - b) <= Math.max(2, 0.05 * Math.max(a, b));
}

function relClose(a: number, b: number): boolean {
  return Math.abs(a - b) <= 0.035;
}

test("@golden @canvas-worlds-v3 worlds-v3 geometry: footprint fixed, body reveals, cloud-left, substrateV2", async ({ ui }) => {
  test.setTimeout(120_000);
  const { page } = ui;

  await page.addInitScript((key) => {
    try {
      window.localStorage.setItem(key, "true");
    } catch {
      // The assertion below surfaces environments without localStorage.
    }
  }, SUBSTRATE_FLAG_KEY);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  expect(await page.evaluate((key) => window.localStorage.getItem(key), SUBSTRATE_FLAG_KEY)).toBe("true");

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace — seed a plan to verify worlds-v3");
    return;
  }
  const probeTaskId = await pickProbeTaskId(page);
  if (!probeTaskId) {
    test.skip(true, "plan has no tasks to probe — worlds-v3 cloud geometry needs at least one task");
    return;
  }
  await focusTaskById(page, probeTaskId);
  if (!(await screenBox(page, "canvas-task-card", probeTaskId))) {
    test.skip(true, "probe task did not mount after canvas-focus — check the focus contract");
    return;
  }

  await focusTaskById(page, probeTaskId);
  const taskFootprints: Array<{ w: number; h: number }> = [];
  for (const s of [1.6, 2.4, 3.2, 4.5, 6.0]) {
    await setCanvasScaleOnTask(page, probeTaskId, s);
    await waitForCanvasSettled(page);
    const fp = await worldFootprint(page, "canvas-task-card", probeTaskId);
    if (fp) taskFootprints.push(fp);
  }
  await captureBoard(page, "worlds-v3-footprint");
  expect(taskFootprints.length, "task world footprint sampled across the substrate dive").toBeGreaterThanOrEqual(2);
  for (let i = 1; i < taskFootprints.length; i++) {
    expect(footprintEqual(taskFootprints[i].w, taskFootprints[0].w), `task world width fixed (${JSON.stringify(taskFootprints)})`).toBe(true);
    expect(footprintEqual(taskFootprints[i].h, taskFootprints[0].h), `task world height fixed (${JSON.stringify(taskFootprints)})`).toBe(true);
  }

  const probeBodyVisible = async (): Promise<boolean> =>
    page.evaluate((id) => {
      const card = document.querySelector(`[data-testid="canvas-task-card"][data-task-id="${id}"]`);
      const body = card?.querySelector('[data-testid="canvas-task-body"]') as HTMLElement | null;
      if (!body) return false;
      const cs = getComputedStyle(body);
      if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) < 0.05) return false;
      const r = body.getBoundingClientRect();
      return r.width > 2 && r.height > 2;
    }, probeTaskId);

  await focusTaskById(page, probeTaskId);
  let taskBody = await probeBodyVisible();
  for (let i = 0; i < 14 && !taskBody; i++) {
    await zoomTowardTask(page, probeTaskId, -160);
    taskBody = await probeBodyVisible();
  }
  await captureBoard(page, "worlds-v3-body");
  // reveals at the W2 content band — reachable by zoom alone, content-independent.
  expect(taskBody, "canvas-task-body revealed by zoom into the W2 content band (§35.3)").toBe(true);

  // Anchor every step on the probe task so it never drifts off-screen / gets culled.
  const diveScales = [1.0, 1.3, 1.6, 2.0, 2.4, 3.0, 3.6, 4.5, 5.5, 7.5, 10];
  const snapshots: Array<NonNullable<Awaited<ReturnType<typeof taskDiveSnapshot>>> & { scale: number }> = [];
  await focusTaskById(page, probeTaskId);
  for (const s of diveScales) {
    await setCanvasScaleOnTask(page, probeTaskId, s);
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

  const readSnaps = snapshots.filter((s) => s.world === "W2" || s.world === "W3");
  const baseline = readSnaps.find((s) => s.boxes.name && s.boxes.meta && s.boxes.summary && s.boxes.bodySlot && s.boxes.strides);
  expect(baseline, "has a complete inner-slot baseline sample in the W2+ read regime").toBeTruthy();
  if (baseline) {
    for (const snap of readSnaps) {
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

  const scaleNow = await canvasScale(page);
  if (scaleNow > 1.05) {
    const crispWrappers = await page.locator(".ct-canvas-crisp").count();
    expect(crispWrappers, "no per-card .ct-canvas-crisp wrapper under substrateV2").toBe(0);
  }

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
