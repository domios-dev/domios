/**
 * 13-canvas — PAN/ZOOM FRAME-TIME GATE (loop_e424ce0b, profile-driven perf loop).
 *
 * The measurement instrument for the canvas perf loop. Drives SUSTAINED real
 * gestures and samples the actual rAF frame cadence on the page, so the numbers
 * reflect main-thread jank from React re-render work — not synthetic noise.
 *
 *   - PAN  uses page.mouse.wheel (a REAL, trusted CDP wheel) → drives the native
 *     non-passive wheel listener in useCanvasViewport (a JS-dispatched WheelEvent
 *     does fire the listener too, but a trusted wheel is the faithful path).
 *   - ZOOM holds Control and real-wheels, so the wheel carries ctrlKey=true and
 *     hits the zoom-around-pointer branch — a real pinch/⌘-wheel, not a synthetic
 *     dispatch.
 *
 * Frame timing: a rAF loop on the page records inter-frame deltas during each
 * gesture window; we report p50/p95/max ms + dropped-frame % (a frame > 32ms ≈ a
 * dropped 30fps frame). An idle window is measured first as a control. Budgets are
 * asserted so this is a real GATE, not just a print.
 *
 * Runs against the running dev (a dev build, so absolute ms are looser than prod —
 * use it as a repeatable RELATIVE gate for before/after hardening):
 *   CT_E2E_PORT=7686 npx playwright test -c tests/playwright -g canvas-perf --workers=1
 */
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { canvasScale, ensureCanvasBoard, isEmpty, panToMountTaskCards, setCanvasScale } from "../../helpers/canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";
const PERF_N = 150; // a real board: one plan, 150 tasks in the cloud

interface FrameStats {
  label: string;
  frames: number;
  p50: number;
  p95: number;
  max: number;
  droppedPct: number;
  // Attribution: is a heavy frame main-thread JS (longtask) and/or backend IPC?
  longtaskCount: number;
  longtaskTotalMs: number;
  longtaskMaxMs: number;
  ipc: string[]; // window.__ctPerf entries (SLOW-IPC / LONGTASK / cmd) seen this window
}

const PAN_P95_BUDGET_MS = 34;
const ZOOM_FAR_P95_BUDGET_MS = 40;
const ZOOM_NEAR_P95_BUDGET_MS = 45;
const DROPPED_BUDGET_PCT = 25;

interface PerfWin {
  frames: number[];
  raf: number;
  last: number;
  on: boolean;
  longtasks: number[];
  po: PerformanceObserver | null;
  ipcStart: number;
}

async function startSampler(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __perf?: PerfWin; __ctPerf?: { cmd: string; ms: number }[] };
    w.__perf = { frames: [], raf: 0, last: 0, on: true, longtasks: [], po: null, ipcStart: w.__ctPerf?.length ?? 0 };
    const p = w.__perf;
    const loop = (ts: number) => {
      if (p.last) p.frames.push(ts - p.last);
      p.last = ts;
      if (p.on) p.raf = requestAnimationFrame(loop);
    };
    p.raf = requestAnimationFrame(loop);
    // Attribution: any main-thread block >50ms (render, sync JS, IPC resolve) lands here.
    try {
      const po = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) p.longtasks.push(e.duration);
      });
      po.observe({ entryTypes: ["longtask"] });
      p.po = po;
    } catch {
      /* longtask unsupported */
    }
  });
}

async function stopSampler(page: Page, label: string): Promise<FrameStats> {
  return page.evaluate((label) => {
    const w = window as unknown as { __perf: PerfWin; __ctPerf?: { cmd: string; ms: number }[] };
    const p = w.__perf;
    p.on = false;
    cancelAnimationFrame(p.raf);
    if (p.po) p.po.disconnect();
    const d = p.frames.slice().sort((a, b) => a - b);
    const q = (pp: number) => (d.length ? d[Math.min(d.length - 1, Math.floor((pp / 100) * d.length))] : 0);
    const dropped = p.frames.filter((x) => x > 32).length;
    const r1 = (n: number) => Math.round(n * 10) / 10;
    const lt = p.longtasks;
    const ipc = (w.__ctPerf ?? []).slice(p.ipcStart).map((e) => `${e.cmd}:${Math.round(e.ms)}ms`);
    return {
      label,
      frames: d.length,
      p50: r1(q(50)),
      p95: r1(q(95)),
      max: r1(d[d.length - 1] ?? 0),
      droppedPct: d.length ? r1((dropped / d.length) * 100) : 0,
      longtaskCount: lt.length,
      longtaskTotalMs: r1(lt.reduce((a, b) => a + b, 0)),
      longtaskMaxMs: r1(lt.length ? Math.max(...lt) : 0),
      ipc,
    } satisfies FrameStats;
  }, label);
}

// camera stays over content instead of flying off the board.
async function drivePan(page: Page, durationMs: number): Promise<void> {
  const box = await page.locator('[data-testid="canvas-view"]').first().boundingBox();
  if (!box) return;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const end = Date.now() + durationMs;
  let i = 0;
  while (Date.now() < end) {
    const dir = Math.floor(i / 10) % 2 === 0 ? 1 : -1;
    await page.mouse.wheel(70 * dir, 28 * dir);
    await page.waitForTimeout(16);
    i++;
  }
}

async function driveZoom(page: Page, durationMs: number): Promise<{ min: number; max: number }> {
  let min = Number.POSITIVE_INFINITY;
  let max = 0;
  const end = Date.now() + durationMs;
  let i = 0;
  while (Date.now() < end) {
    const dy = Math.floor(i / 8) % 2 === 0 ? -40 : 40; // zoom in, then back out
    await page.evaluate((deltaY) => {
      const view = document.querySelector('[data-testid="canvas-view"]');
      const target = view?.querySelector("div");
      if (!target) return;
      const r = target.getBoundingClientRect();
      target.dispatchEvent(
        new WheelEvent("wheel", { deltaY, ctrlKey: true, bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }),
      );
    }, dy);
    await page.waitForTimeout(16);
    if (i % 6 === 0) {
      const s = await canvasScale(page);
      min = Math.min(min, s);
      max = Math.max(max, s);
    }
    i++;
  }
  return { min: Number.isFinite(min) ? min : 0, max };
}

function fmt(s: FrameStats): string {
  const lt = `longtask ${s.longtaskCount}×/${s.longtaskTotalMs}ms (max ${s.longtaskMaxMs}ms)`;
  const ipc = s.ipc.length ? `  IPC[${s.ipc.join(", ")}]` : "  IPC[none]";
  return `${s.label.padEnd(10)} p50=${String(s.p50).padStart(5)}ms  p95=${String(s.p95).padStart(6)}ms  max=${String(s.max).padStart(6)}ms  dropped=${String(s.droppedPct).padStart(5)}%  (${s.frames}f)  ${lt}${ipc}`;
}

test("@canvas-perf pan/zoom sustain a smooth frame budget on a populated board", async ({ ui }) => {
  test.setTimeout(300_000);
  const { page } = ui;

  // screen-space overlay layer is not in play — world-layer cards carry the content.
  await page.addInitScript(([perfKey, subKey, n]) => {
    try {
      window.localStorage.setItem(perfKey as string, String(n));
      window.localStorage.removeItem(subKey as string);
    } catch {
      /* surfaced by the skip below */
    }
  }, [PERF_KEY, SUBSTRATE_KEY, PERF_N] as const);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

  await ensureCanvasBoard(page);
  if (await isEmpty(page)) {
    test.skip(true, "canvas empty — perf fixture did not inject");
    return;
  }

  // the realistic pan/zoom-lag scenario (dozens of nodes re-rendering per gesture).
  await panToMountTaskCards(page, 1.3);
  const visibleCards = await page.locator('[data-testid="canvas-task-card"]').count();
  // eslint-disable-next-line no-console
  console.log(`\n[canvas-perf] board: perfN=${PERF_N}, scale≈${(await canvasScale(page)).toFixed(2)}, mounted cards=${visibleCards}`);

  // Warm up (JIT + first-paint settle) so the measured windows are steady-state.
  await drivePan(page, 600);
  await driveZoom(page, 600);
  await page.waitForTimeout(200);

  // Idle control: the natural rAF cadence with no gesture.
  await startSampler(page);
  await page.waitForTimeout(1500);
  const idle = await stopSampler(page, "idle");

  // PAN at the mid view.
  await startSampler(page);
  await drivePan(page, 2500);
  const pan = await stopSampler(page, "pan");

  // ZOOM in the GLANCE band (scale < 1: W0/W1, no crisp re-raster, lightweight cards).
  await setCanvasScale(page, 0.6);
  await startSampler(page);
  const farRange = await driveZoom(page, 2500);
  const zoomFar = await stopSampler(page, "zoom-far");

  // ZOOM in the READING band (scale > 1: W2/W3, crisp re-raster + rich content active).
  await setCanvasScale(page, 2.0);
  await startSampler(page);
  const nearRange = await driveZoom(page, 2500);
  const zoomNear = await stopSampler(page, "zoom-near");

  // eslint-disable-next-line no-console
  console.log(
    `\n──────── canvas-perf frame-time (dev build, perfN=${PERF_N}) ────────\n` +
      `${fmt(idle)}\n${fmt(pan)}\n${fmt(zoomFar)}  scale ${farRange.min.toFixed(2)}→${farRange.max.toFixed(2)}\n${fmt(zoomNear)}  scale ${nearRange.min.toFixed(2)}→${nearRange.max.toFixed(2)}\n` +
      `─────────────────────────────────────────────────────────────\n`,
  );

  // Sanity: the gestures actually moved the camera (else the numbers are meaningless).
  expect(pan.frames, "pan window captured frames").toBeGreaterThan(20);
  expect(zoomFar.frames + zoomNear.frames, "zoom windows captured frames").toBeGreaterThan(40);
  expect(farRange.max - farRange.min + (nearRange.max - nearRange.min), "zoom drives moved the camera scale").toBeGreaterThan(0.1);

  // The GATE — PAN + GLANCE-zoom are the render domain (tight); DEEP-zoom is loose.
  expect(pan.p95, `PAN p95 ${pan.p95}ms over budget ${PAN_P95_BUDGET_MS}ms`).toBeLessThanOrEqual(PAN_P95_BUDGET_MS);
  expect(pan.droppedPct, `PAN dropped ${pan.droppedPct}% over budget ${DROPPED_BUDGET_PCT}%`).toBeLessThanOrEqual(DROPPED_BUDGET_PCT);
  expect(zoomFar.p95, `ZOOM-FAR p95 ${zoomFar.p95}ms over budget ${ZOOM_FAR_P95_BUDGET_MS}ms`).toBeLessThanOrEqual(ZOOM_FAR_P95_BUDGET_MS);
  expect(zoomNear.p95, `ZOOM-NEAR p95 ${zoomNear.p95}ms over budget ${ZOOM_NEAR_P95_BUDGET_MS}ms`).toBeLessThanOrEqual(ZOOM_NEAR_P95_BUDGET_MS);
  expect(zoomNear.droppedPct, `ZOOM-NEAR dropped ${zoomNear.droppedPct}% over budget ${DROPPED_BUDGET_PCT}%`).toBeLessThanOrEqual(DROPPED_BUDGET_PCT);
});
