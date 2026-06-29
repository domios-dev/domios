import { expect, test } from "../../helpers/fixtures";
import {
  formatBaselineTable,
  formatThresholdTable,
  installPerfFixture,
  l5Thresholds,
  measureCanvasBaseline,
  type CanvasBaselineSample,
} from "../../helpers/canvas-perf";

const FIXTURE_NODE_COUNTS = [100, 500, 1000] as const;

test("@golden @canvas-perf-baseline records deterministic synthetic canvas baseline", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;
  await installPerfFixture(page, FIXTURE_NODE_COUNTS[0]);

  const samples: CanvasBaselineSample[] = [];
  for (const nodes of FIXTURE_NODE_COUNTS) {
    const sample = await measureCanvasBaseline(page, nodes);
    samples.push(sample);
    expect(sample.worldCanvasCount, `${nodes} fixture should mount the blit canvas`).toBe(1);
    expect(sample.presenceOverlayCount, `${nodes} fixture should mount synthetic live overlays`).toBe(sample.presencePinCount);
    expect(
      sample.presencePinCount + sample.swarmMemberCount + sample.liveAgentTextCount,
      `${nodes} fixture should expose synthetic live agents`,
    ).toBeGreaterThan(0);
    expect(sample.pan.frames, `${nodes} pan frame sampler`).toBeGreaterThan(20);
    expect(sample.zoom.frames, `${nodes} zoom frame sampler`).toBeGreaterThan(20);
  }

  const thresholds = l5Thresholds(samples);
  console.log(`\n[canvas-perf-baseline]\n${formatBaselineTable(samples)}\n`);
  console.log(`[canvas-perf-baseline L5 thresholds]\n${formatThresholdTable(thresholds)}\n`);
});
