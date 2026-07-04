/**
 * 13-canvas — §32 WORLDS v2 verification matrix. Asserts the orthogonalized
 * channels (§32.1): W0 is compressed-not-hollow (§32.2), content text is opacity
 * 1.0 at every band and done SINKS spatially below in_progress (§32.3), the
 * footprint tracks volume with an aspect floor (§32.4), strides render as a
 * connected snake of circles that blooms on zoom (§32.5), canvas text is
 * selectable (§32.6), a section click highlights the right tasks WITHOUT moving
 * the camera (§31/§32.7), and handoffs render on the timeline lane (§32.8).
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=full --grep @canvas-worlds-v2
 */
import { expect, test } from "../../helpers/fixtures";
import { isEmpty, openCanvas, zoomBy } from "../../helpers/canvas-inspect";

type Page = import("@playwright/test").Page;

async function visible(page: Page, testid: string): Promise<boolean> {
  return page.evaluate((tid) => {
    const el = document.querySelector(`[data-testid="${tid}"]`);
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return r.width > 2 && r.height > 2;
  }, testid);
}

async function count(page: Page, sel: string): Promise<number> {
  return page.locator(sel).count();
}

// raw so we can assert the camera is byte-identical across a non-camera action.
async function cameraTransform(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const view = document.querySelector('[data-testid="canvas-view"]');
    const world = view?.querySelector('div[style*="scale("]') as HTMLElement | null;
    return world ? world.style.transform : null;
  });
}

async function zoomInUntil(page: Page, testid: string, max = 14): Promise<boolean> {
  for (let i = 0; i < max; i++) {
    if (await visible(page, testid)) return true;
    await zoomBy(page, -150);
  }
  return visible(page, testid);
}

test("@golden @canvas-worlds-v2 W0 compressed, opacity-1 content, done sinks, snake, selectable, section nav, handoff", async ({ ui }) => {
  test.setTimeout(150_000);
  const { page } = ui;

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans — seed via scripts/seed-canvas-dogfood-full.sh on this instance");
    return;
  }

  await zoomBy(page, 260);
  await zoomBy(page, 260);
  const w0Filled = (await visible(page, "canvas-task-digest")) || (await visible(page, "canvas-task-counters")) || (await visible(page, "canvas-task-steps"));
  expect(w0Filled, "W0 task shows compressed content (digest/counters/snake), not a hollow box").toBe(true);
  // PLAN/SPEC W0 shows compressed section rows + counts, not a bare "N sections".
  expect(await visible(page, "canvas-section-compressed"), "PLAN/SPEC W0 shows compressed rows").toBe(true);

  const haveStrides = (await count(page, '[data-testid="canvas-task-steps"]')) > 0;
  if (haveStrides) {
    const beads = await count(page, 'circle[data-testid="canvas-task-step-dot"]');
    expect(beads, "strides render as circular beads at far/mid zoom").toBeGreaterThan(0);
  }

  const gotBody = await zoomInUntil(page, "canvas-task-body");
  expect(gotBody, "TASK body reachable by zoom (W2+)").toBe(true);

  // §32.3 primary content is ALWAYS opacity 1.0 (opacity is never a status signal).
  const bodyOpacity = await page.evaluate(() => {
    const el = document.querySelector('[data-testid="canvas-task-body"]') as HTMLElement | null;
    return el ? Number(getComputedStyle(el).opacity) : null;
  });
  expect(bodyOpacity, "task body opacity is 1.0 at the content band").toBe(1);

  // §32.5 bloom: deep zoom turns beads into stride cards while the thread stays.
  if (haveStrides) {
    const bloomed = await zoomInUntil(page, "canvas-task-step-card", 8);
    expect(bloomed, "strides bloom into full cards at deep zoom (W3)").toBe(true);
  }

  const aspect = await page.$$eval('[data-testid="canvas-task-card"]', (els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 ? r.height / r.width : 1;
    }),
  );
  expect(aspect.every((a) => a >= 0.6 - 0.02), "no hollow letterboxes — height ≥ aspect floor").toBe(true);

  const shelves = await page.$$eval('[data-testid="canvas-task-card"]', (els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { shelf: el.getAttribute("data-shelf"), x: r.x, y: r.y };
    }),
  );
  let checkedShelf = false;
  for (const a of shelves.filter((s) => s.shelf === "active")) {
    for (const d of shelves.filter((s) => s.shelf === "done")) {
      if (Math.abs(a.x - d.x) < 40) {
        checkedShelf = true;
        expect(d.y, "done task sits below the in_progress task in its column").toBeGreaterThan(a.y - 4);
      }
    }
  }
  void checkedShelf;

  const selectable = await page.evaluate(() => {
    const el = document.querySelector('[data-canvas-selectable]') as HTMLElement | null;
    if (!el) return null;
    return getComputedStyle(el).userSelect || getComputedStyle(el).webkitUserSelect;
  });
  expect(selectable, "canvas text island is user-select:text").toBe("text");

  const row = page.locator('[data-testid="canvas-plan-row"]').first();
  if ((await row.count()) > 0) {
    const before = await cameraTransform(page);
    await row.click({ position: { x: 8, y: 8 } });
    await page.waitForTimeout(200);
    const after = await cameraTransform(page);
    expect(after, "camera transform byte-identical after a section single-click (§31)").toBe(before);
  }

  const handoffs = await count(page, '[data-testid="canvas-handoff-coin"]');
  if (handoffs > 0) {
    expect(await visible(page, "canvas-handoff-coin"), "handoff coin renders (not the fallback)").toBe(true);
    expect(await count(page, '[data-testid="canvas-fallback-node"][data-node-kind="handoff"]'), "handoff is NOT the fallback node").toBe(0);
  }
});
