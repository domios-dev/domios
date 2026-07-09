/**
 * WHAT: canvas visual covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import {
  assertClean,
  auditCanvas,
  fitToView,
  focusFirstNode,
  isEmpty,
  openCanvas,
  panBy,
  zoomBy,
} from "../../helpers/canvas-inspect";

const ITERS = Number(process.env.CANVAS_ITERS ?? "20");

test("@canvas-visual board opens, renders nodes, and survives a pan/zoom/fit loop without overlap or clipping", async ({ ui }) => {
  test.setTimeout(Math.max(120_000, ITERS * 8_000));
  const { page } = ui;

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace — populate a plan to inspect the board");
    return;
  }

  // Baseline frame.
  const baseline = await auditCanvas(page, "iter-00-baseline");
  expect(baseline.nodes, "at least one plan node on the board").toBeGreaterThan(0);
  assertClean(baseline);

  // pose; no Math.random so the loop is reproducible across runs.
  for (let i = 1; i <= ITERS; i++) {
    const phase = i % 4;
    if (phase === 0) await fitToView(page);
    else if (phase === 1) await panBy(page, 120 + i * 7, -60 - i * 5);
    else if (phase === 2) await zoomBy(page, -120);
    else {
      await zoomBy(page, 140);
      await focusFirstNode(page, "canvas-task-card");
    }

    const label = `iter-${String(i).padStart(2, "0")}-phase${phase}`;
    const result = await auditCanvas(page, label);
    // eslint-disable-next-line no-console
    console.log(`[canvas-visual] ${label}: nodes=${result.nodes} overlaps=${result.overlaps.length} overflows=${result.overflows.length}`);
    assertClean(result);
  }
});
