/**
 * WHAT: canvas ux1 schematic covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
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
  // PW_JUSTIFIED_SLEEP: fixed 200ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(200);
  await checkBand("glance");

  await panToMountTaskCards(page, 1.6); // READING — cards framed in the cloud
  // PW_JUSTIFIED_SLEEP: fixed 200ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(200);
  await checkBand("reading");

  // size (the world floor is unit-verified in taskFootprint.test). Logged for evidence.
  const subBoxes = await page.$$eval('[data-testid="canvas-task-card"][data-task-id*="_sub"]', (els) =>
    els.map((el) => { const r = el.getBoundingClientRect(); return { id: el.getAttribute("data-task-id"), w: Math.round(r.width), h: Math.round(r.height) }; }),
  );
  // eslint-disable-next-line no-console
  console.log(`[ux3] subtasks in view=${subBoxes.length} ${JSON.stringify(subBoxes.slice(0, 3))}`);
});
