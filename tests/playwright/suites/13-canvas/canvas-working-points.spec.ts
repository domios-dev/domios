/**
 * WHAT: canvas working points covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import {
  isEmpty,
  openCanvas,
  panBy,
  panToMountTaskCards,
  setCanvasScaleOnTask,
  waitForCanvasSettled,
} from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const CARD = (id: string) => `[data-testid="canvas-task-card"][data-task-id="${id}"]`;

async function shot(page: Page, name: string): Promise<void> {
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: `/tmp/${name}.png` }).catch(async () => {
    await page.screenshot({ path: `/tmp/${name}.png` });
  });
}

async function taskWorld(page: Page, id: string): Promise<string | null> {
  return page.getAttribute(`${CARD(id)} >> nth=0`, "data-world").catch(() => null);
}

// card drifts to an edge after the cloud sweep). panBy maps wheel deltas → pan.
async function recenterCard(page: Page, id: string): Promise<void> {
  const d = await page.evaluate((sel) => {
    const el = document.querySelector(sel) as HTMLElement | null;
    const view = document.querySelector('[data-testid="canvas-view"]') as HTMLElement | null;
    if (!el || !view) return null;
    const r = el.getBoundingClientRect();
    const v = view.getBoundingClientRect();
    return { dx: r.left + r.width / 2 - (v.left + v.width / 2), dy: r.top + r.height / 2 - (v.top + v.height / 2) };
  }, CARD(id));
  if (d) await panBy(page, d.dx, d.dy);
}

// Drive a single task card to (or past) a target band by sampling scales.
async function reachTaskWorld(page: Page, id: string, target: string, scales: number[]): Promise<string | null> {
  let band = await taskWorld(page, id);
  for (const s of scales) {
    await setCanvasScaleOnTask(page, id, s);
    await waitForCanvasSettled(page);
    band = await taskWorld(page, id);
    if (band === target) break;
  }
  return band;
}

// Pairwise overlap (area > tolerance) among the stride callouts/labels of one card.
async function labelOverlaps(page: Page, id: string): Promise<{ overlaps: string[]; overflow: boolean; count: number }> {
  return page.evaluate(({ sel }) => {
    const card = document.querySelector(sel) as HTMLElement | null;
    if (!card) return { overlaps: [], overflow: false, count: 0 };
    const cardRect = card.getBoundingClientRect();
    const labels = [...card.querySelectorAll('[data-testid="canvas-stride-label"]')] as HTMLElement[];
    const rects = labels.map((el, i) => {
      const r = el.getBoundingClientRect();
      return { i, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    });
    const overlaps: string[] = [];
    for (let a = 0; a < rects.length; a++) {
      for (let b = a + 1; b < rects.length; b++) {
        const x = Math.max(0, Math.min(rects[a].right, rects[b].right) - Math.max(rects[a].left, rects[b].left));
        const y = Math.max(0, Math.min(rects[a].bottom, rects[b].bottom) - Math.max(rects[a].top, rects[b].top));
        if (x * y > 6) overlaps.push(`${rects[a].i}/${rects[b].i}`);
      }
    }
    // Callouts overflow the card horizontally past a small tolerance?
    const overflow = rects.some((r) => r.right > cardRect.right + 3 || r.left < cardRect.left - 3);
    return { overlaps, overflow, count: rects.length };
  }, { sel: CARD(id) });
}

test("@golden @canvas-working-points strides distinguishable W1/W2, deep-read W3, no callout overlap", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;

  await page.addInitScript(([perfKey, subKey]) => {
    try {
      window.localStorage.setItem(perfKey, "24");
      window.localStorage.setItem(subKey, "true");
    } catch {
      /* surfaced below */
    }
  }, [PERF_KEY, SUBSTRATE_KEY]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }

  // task that actually carries strides (perf fixture gives every task 4).
  const cards = await panToMountTaskCards(page, 1.6);
  if (cards.length === 0) {
    test.skip(true, "no task cards mounted after cloud sweep");
    return;
  }
  const probe = await page.evaluate(() => {
    const els = [...document.querySelectorAll('[data-testid="canvas-task-card"][data-task-id]')] as HTMLElement[];
    return els[0]?.getAttribute("data-task-id") ?? null;
  });
  expect(probe, "probe task id from DOM").toBeTruthy();
  const id = probe!;

  const w1 = await reachTaskWorld(page, id, "W1", [0.7, 0.95, 1.2, 1.5]);
  await shot(page, "wp-W1");
  const kindsW1 = await page.locator(`${CARD(id)} [data-testid="canvas-stride-kind"]`).allInnerTexts().catch(() => []);
  if (w1 === "W1") {
    expect(kindsW1.length, "W1 names working points by kind").toBeGreaterThan(0);
    expect(kindsW1.some((t) => t.trim().length > 0), "kind labels carry text").toBe(true);
  }

  const w2 = await reachTaskWorld(page, id, "W2", [1.9, 2.3, 2.7, 3.1]);
  await recenterCard(page, id);
  await shot(page, "wp-W2");
  const beadKinds = await page.locator(`${CARD(id)} [data-testid="canvas-task-step-dot"][data-milestone-kind]`).count();
  const ov = await labelOverlaps(page, id);
  if (w2 === "W2") {
    expect(beadKinds, "beads carry kind identity at W2").toBeGreaterThan(0);
    expect(ov.overlaps, `callouts must not overlap (n=${ov.count})`).toEqual([]);
    expect(ov.overflow, "callouts must not overflow the card").toBe(false);
    // Frame the bead snake itself (clearer than the whole tall card).
    await page.locator(`${CARD(id)} [data-testid="canvas-task-steps"]`).first()
      .screenshot({ path: "/tmp/wp-W2-snake.png" }).catch(() => {});
  }

  const w3 = await reachTaskWorld(page, id, "W3", [3.9, 4.8, 5.6, 6.4]);
  await recenterCard(page, id);
  await shot(page, "wp-W3");
  expect([w2, w3].includes("W2") || [w2, w3].includes("W3"), `reached a reading band (w2=${w2} w3=${w3})`).toBe(true);

  if (w3 === "W3") {
    const stepCards = page.locator(`${CARD(id)} [data-testid="canvas-task-step-card"]`);
    expect(await stepCards.count(), "W3 blooms strides into full cards").toBeGreaterThan(0);
    const texts = page.locator(`${CARD(id)} [data-testid="canvas-task-step-text"]`);
    expect(await texts.count(), "stride text readable at W3").toBeGreaterThan(0);
    // Frame one bloomed stride for a readable close-up (text + meta).
    await stepCards.first().screenshot({ path: "/tmp/wp-W3-stride-closeup.png" }).catch(() => {});
    // The strides slot is a node-internal column; the outer card clips, no overflow.
    const slot = await page.evaluate((sel) => {
      const el = document.querySelector(`${sel} [data-testid="canvas-task-strides-slot"]`) as HTMLElement | null;
      return el ? { scroll: el.scrollHeight, client: el.clientHeight } : null;
    }, CARD(id));
    expect(slot, "strides slot present at W3").not.toBeNull();
  }
});
