/**
 * 13-canvas — TASK CARD FULL TITLES + readable sizing (north-star #2).
 *
 * Two real complaints, two assertions:
 *   1. FULL TITLES — at the reading bands (W1/W2/W3) a task card shows its WHOLE
 *      title (wrapped to as many lines as it needs), never clipped, never an
 *      ellipsis. The footprint reserves the height; the card never cuts its name.
 *      Only the far W0 mass-glance keeps a 2-line ellipsis cue.
 *   2. READABLE SIZING — cards are big enough to hold their visible-band content:
 *      zero overflow on every mounted card across the dive.
 *
 * Driven against the perf fixture, whose synthetic tasks carry deliberately long
 * (~110-char) titles — the worst case for wrapping. Skips on an empty board.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g @canvas-task-titles --retries=0 --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import {
  collectBoxes,
  findVisibleOverflows,
  focusTaskById,
  isEmpty,
  openCanvas,
  panBy,
  panToMountTaskCards,
} from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";

async function titleClipReport(page: Page) {
  return page.evaluate(() => {
    const cards = Array.from(
      document.querySelectorAll('[data-testid="canvas-task-card"]'),
    ) as HTMLElement[];
    const out: Array<{
      id: string | null;
      world: string | null;
      clipped: boolean;
      ellipsis: boolean;
      lines: number;
      full: string;
    }> = [];
    for (const card of cards) {
      const r = card.getBoundingClientRect();
      if (r.width < 4 || r.height < 4) continue;
      const title = card.querySelector('[data-testid="canvas-task-name"]') as HTMLElement | null;
      if (!title) continue;
      const tr = title.getBoundingClientRect();
      const cs = getComputedStyle(title);
      // -webkit-line-clamp renders the ellipsis cue; block (W1+) has no clamp.
      const ellipsis = cs.webkitLineClamp !== "none" && cs.webkitLineClamp !== "";
      const lines = Math.round(tr.height / (parseFloat(cs.lineHeight) || 16));
      out.push({
        id: card.getAttribute("data-task-id"),
        world: card.getAttribute("data-world"),
        // 2px tolerance for sub-pixel rounding of the card border.
        clipped: tr.bottom > r.bottom + 2 || tr.top < r.top - 2,
        ellipsis,
        lines,
        full: title.getAttribute("data-task-name-full") ?? "",
      });
    }
    return out;
  });
}

async function shot(page: Page, name: string): Promise<void> {
  await page
    .locator('[data-testid="canvas-view"]')
    .first()
    .screenshot({ path: `/tmp/${name}.png` })
    .catch(() => {});
}

test("@golden @canvas-task-titles task cards show full titles, zero overflow, across reading bands", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;

  await page.addInitScript(([perfKey, subKey]) => {
    try {
      window.localStorage.setItem(perfKey, "24");
      window.localStorage.setItem(subKey, "true");
    } catch {
      /* surfaced by the skip below */
    }
  }, [PERF_KEY, SUBSTRATE_KEY]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }

  // Mount task cards (the cloud sits off-screen right of the plan after fit-to-view).
  const mounted = await panToMountTaskCards(page, 1.6);
  if (mounted.length === 0) {
    test.skip(true, "no task cards mounted after pan sweep");
    return;
  }

  const probe = mounted[0].id;
  expect(probe, "probe task id present").toBeTruthy();

  // together, then capture — a readable-sizing eyeball of the cards side by side.
  for (let i = 0; i < 4; i++) {
    await panBy(page, -360, -120);
    if ((await collectBoxes(page, "canvas-task-card")).length >= 3) break;
  }
  await page.waitForTimeout(200);
  await shot(page, "task-titles-cloud-multi");

  const seenBands = new Set<string>();
  for (const scale of [1.0, 1.8, 2.6, 3.6, 5.0]) {
    await focusTaskById(page, probe!, scale);
    await page.waitForTimeout(220);

    const report = (await titleClipReport(page)).filter((r) => r.world);
    if (report.length === 0) continue;

    // ellipsis cue (that is a W0-only mass-glance affordance).
    const reading = report.filter((r) => r.world === "W1" || r.world === "W2" || r.world === "W3");
    for (const r of reading) {
      seenBands.add(r.world!);
      expect(r.clipped, `title not clipped (${r.world} ${r.id}, ${r.lines} lines): ${r.full}`).toBe(false);
      expect(r.ellipsis, `no ellipsis at ${r.world} (${r.id}): ${r.full}`).toBe(false);
    }
    // W0 mass-glance keeps the 2-line ellipsis cue (allowed there only).
    for (const r of report.filter((x) => x.world === "W0")) {
      expect(r.clipped, `W0 title still within card box (${r.id})`).toBe(false);
    }

    const overflows = await findVisibleOverflows(page);
    expect(overflows, `zero overflow on cards @scale ${scale}: ${JSON.stringify(overflows)}`).toEqual([]);

    if (scale === 1.0) await shot(page, "task-titles-W0-W1");
    if (scale === 1.8 || scale === 2.6) await shot(page, "task-titles-W2");
    if (scale >= 3.6) await shot(page, "task-titles-W3");
  }

  // At least one true reading band was exercised with full, unclipped titles.
  expect(
    seenBands.has("W1") || seenBands.has("W2") || seenBands.has("W3"),
    `reached a reading band (saw ${[...seenBands].join(",") || "none"})`,
  ).toBe(true);

  console.log("task-titles bands seen:", [...seenBands].join(","), "— screenshots in /tmp/task-titles-*.png");
});
