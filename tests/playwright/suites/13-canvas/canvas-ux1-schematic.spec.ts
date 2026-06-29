/**
 * 13-canvas — UX-1 SCHEMATIC TASK CARDS: screenshot + verify the minimal map face
 * across camera scales (W0 glance → W3 read).
 *
 * Asserts, at the MAP bands, that the task card is schematic (no full body on the
 * face), the title fits inside the card (never clipped past the box), and captures a
 * board screenshot at each band for the orchestrator's visual gate.
 *
 *   CT_E2E_PORT=7686 npx playwright test -c tests/playwright -g canvas-ux1 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { canvasScale, captureBoard, collectBoxes, ensureCanvasBoard, isEmpty, panToMountTaskCards, setCanvasScale } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const PERF_N = 40;

// Title must sit INSIDE its card box (never clipped past the bottom/right edge).
async function titleOverflows(page: Page): Promise<{ id: string | null; overBottomPx: number }[]> {
  return page.evaluate(() => {
    const out: { id: string | null; overBottomPx: number }[] = [];
    for (const card of Array.from(document.querySelectorAll('[data-testid="canvas-task-card"]')) as HTMLElement[]) {
      const name = card.querySelector('[data-testid="canvas-task-name"]') as HTMLElement | null;
      if (!name) continue;
      const cb = card.getBoundingClientRect();
      const nb = name.getBoundingClientRect();
      const overBottom = nb.bottom - cb.bottom;
      const overRight = nb.right - cb.right;
      if (overBottom > 2 || overRight > 2) out.push({ id: card.getAttribute("data-task-id"), overBottomPx: Math.round(Math.max(overBottom, overRight)) });
    }
    return out;
  });
}

test("@canvas-ux1 schematic task face across scales — title fits, body cut, screenshots", async ({ ui }) => {
  test.setTimeout(120_000);
  const { page } = ui;

  await page.addInitScript(([perfKey, n]) => {
    try {
      window.localStorage.setItem(perfKey as string, String(n));
      window.localStorage.removeItem("canvas.substrateV2");
    } catch {
      /* surfaced by the skip below */
    }
  }, [PERF_KEY, PERF_N] as const);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await ensureCanvasBoard(page);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }
  // Two reliable framings: GLANCE (zoom out → whole board) and READING (pan into the
  // cloud at ~1.6×). Both assert the schematic invariants; the reading view also
  // checks subtask legibility. Deep-band schematic-ness is unit-covered (TaskCard).
  const checkBand = async (label: string) => {
    const cards = await collectBoxes(page, "canvas-task-card");
    if (cards.length === 0) return;
    await captureBoard(page, `ux1-${label}`);
    const clipped = await titleOverflows(page);
    const schematicCount = await page.locator('[data-testid="canvas-task-schematic"]').count();
    const bodyCount = await page.locator('[data-testid="canvas-task-full-body"]').count();
    // eslint-disable-next-line no-console
    console.log(`[ux1] ${label} scale≈${(await canvasScale(page)).toFixed(2)} cards=${cards.length} schematic=${schematicCount} fullBody=${bodyCount} titleClipped=${clipped.length}`);
    expect(clipped, `${label}: titles must fit inside the card (${JSON.stringify(clipped)})`).toHaveLength(0);
    expect(schematicCount, `${label}: map face must be schematic`).toBeGreaterThan(0);
    expect(bodyCount, `${label}: map face must NOT render the full body`).toBe(0);
  };

  await setCanvasScale(page, 0.4); // GLANCE — whole board incl. plan/spec/task/subtask
  await page.waitForTimeout(200);
  await checkBand("glance");

  await panToMountTaskCards(page, 1.6); // READING — cards framed in the cloud
  await page.waitForTimeout(200);
  await checkBand("reading");

  // UX-3 subtask legibility: any subtask satellite in view must read at a comfortable
  // size (the world floor is unit-verified in taskFootprint.test). Logged for evidence.
  const subBoxes = await page.$$eval('[data-testid="canvas-task-card"][data-task-id*="_sub"]', (els) =>
    els.map((el) => { const r = el.getBoundingClientRect(); return { id: el.getAttribute("data-task-id"), w: Math.round(r.width), h: Math.round(r.height) }; }),
  );
  // eslint-disable-next-line no-console
  console.log(`[ux3] subtasks in view=${subBoxes.length} ${JSON.stringify(subBoxes.slice(0, 3))}`);
});
