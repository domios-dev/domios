import type { Page } from "@playwright/test";
import {
  canvasScale,
  waitForCanvasSettled,
  ensureCanvasBoard,
  isEmpty,
  panToMountTaskCards,
  setCanvasScale,
} from "./canvas-inspect";

const PERF_KEY = "canvas.perfFixtureN";
const SUBSTRATE_KEY = "canvas.substrateV2";

export interface FrameWindowStats {
  frames: number;
  p50: number;
  p95: number;
  max: number;
  droppedPct: number;
  longtaskCount: number;
  longtaskTotalMs: number;
  longtaskMaxMs: number;
  churn: CanvasChurnSnapshot;
}

export interface CanvasChurnSnapshot {
  canvasCommits: number;
  cameraPaints: number;
  sceneBuilds: number;
  sceneReadbackPublishes: number;
  renderReadbackBuilds: number;
  presenceSourceBuilds: number;
  presenceOverlayBuilds: number;
  swarmRosterBuilds: number;
  canvasBlits: number;
  canvasResharpens: number;
}

export interface CanvasBaselineSample {
  fixtureNodes: number;
  scale: number;
  worldCanvasCount: number;
  worldCardDomCount: number;
  presencePinCount: number;
  presenceOverlayCount: number;
  swarmMemberCount: number;
  liveAgentTextCount: number;
  idle: FrameWindowStats;
  pan: FrameWindowStats;
  zoom: FrameWindowStats;
}

export interface CanvasBaselineThreshold {
  fixtureNodes: number;
  panP95Ms: number;
  zoomP95Ms: number;
  longtaskCount: number;
}

interface PerfWin {
  frames: number[];
  raf: number;
  last: number;
  on: boolean;
  longtasks: number[];
  po: PerformanceObserver | null;
}

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

function thresholdFromP95(ms: number): number {
  return round1(Math.max(16.7, ms * 0.8));
}

export async function installPerfFixture(page: Page, fixtureNodes: number): Promise<void> {
  await page.addInitScript(([perfKey, subKey, n]) => {
    window.localStorage.setItem(perfKey as string, String(n));
    window.localStorage.setItem(subKey as string, "true");
  }, [PERF_KEY, SUBSTRATE_KEY, fixtureNodes] as const);
}

export async function loadPerfCanvas(page: Page, fixtureNodes: number): Promise<boolean> {
  await page.evaluate(([perfKey, subKey, n]) => {
    window.localStorage.setItem(perfKey as string, String(n));
    window.localStorage.setItem(subKey as string, "true");
  }, [PERF_KEY, SUBSTRATE_KEY, fixtureNodes] as const);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await ensureCanvasBoard(page);
  return !(await isEmpty(page));
}

async function startSampler(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as {
      __canvasPerf?: PerfWin;
      __codetermCanvasChurn?: { reset: () => void };
    };
    w.__codetermCanvasChurn?.reset();
    w.__canvasPerf = { frames: [], raf: 0, last: 0, on: true, longtasks: [], po: null };
    const p = w.__canvasPerf;
    const loop = (ts: number) => {
      if (p.last) p.frames.push(ts - p.last);
      p.last = ts;
      if (p.on) p.raf = requestAnimationFrame(loop);
    };
    p.raf = requestAnimationFrame(loop);
    try {
      const po = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) p.longtasks.push(e.duration);
      });
      po.observe({ entryTypes: ["longtask"] });
      p.po = po;
    } catch {}
  });
}

async function stopSampler(page: Page): Promise<FrameWindowStats> {
  return page.evaluate(() => {
    const w = window as unknown as {
      __canvasPerf: PerfWin;
      __codetermCanvasChurn?: { snapshot: () => CanvasChurnSnapshot };
    };
    const p = w.__canvasPerf;
    p.on = false;
    cancelAnimationFrame(p.raf);
    p.po?.disconnect();
    const sorted = p.frames.slice().sort((a, b) => a - b);
    const q = (pct: number) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor((pct / 100) * sorted.length))] : 0;
    const dropped = p.frames.filter((x) => x > 32).length;
    const longtasks = p.longtasks;
    return {
      frames: sorted.length,
      p50: Math.round(q(50) * 10) / 10,
      p95: Math.round(q(95) * 10) / 10,
      max: Math.round((sorted[sorted.length - 1] ?? 0) * 10) / 10,
      droppedPct: sorted.length ? Math.round((dropped / sorted.length) * 1000) / 10 : 0,
      longtaskCount: longtasks.length,
      longtaskTotalMs: Math.round(longtasks.reduce((a, b) => a + b, 0) * 10) / 10,
      longtaskMaxMs: Math.round((longtasks.length ? Math.max(...longtasks) : 0) * 10) / 10,
      churn: w.__codetermCanvasChurn?.snapshot() ?? {
        canvasCommits: 0,
        cameraPaints: 0,
        sceneBuilds: 0,
        sceneReadbackPublishes: 0,
        renderReadbackBuilds: 0,
        presenceSourceBuilds: 0,
        presenceOverlayBuilds: 0,
        swarmRosterBuilds: 0,
        canvasBlits: 0,
        canvasResharpens: 0,
      },
    };
  });
}

async function drivePan(page: Page, durationMs: number): Promise<void> {
  const box = await page.locator('[data-testid="canvas-view"]').first().boundingBox();
  if (!box) return;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const end = Date.now() + durationMs;
  let i = 0;
  while (Date.now() < end) {
    const dir = Math.floor(i / 8) % 2 === 0 ? 1 : -1;
    await page.mouse.wheel(90 * dir, 36 * dir);
    // PW_JUSTIFIED_SLEEP: stimulus — input pacing IS the pan gesture being measured.
    await page.waitForTimeout(16);
    i++;
  }
}

async function driveZoom(page: Page, durationMs: number): Promise<void> {
  const end = Date.now() + durationMs;
  let i = 0;
  while (Date.now() < end) {
    const deltaY = Math.floor(i / 8) % 2 === 0 ? -44 : 44;
    await page.evaluate((dy) => {
      const view = document.querySelector('[data-testid="canvas-view"]');
      const target = view?.querySelector("div");
      if (!target) return;
      const r = target.getBoundingClientRect();
      target.dispatchEvent(new WheelEvent("wheel", {
        deltaY: dy,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + r.width / 2,
        clientY: r.top + r.height / 2,
      }));
    }, deltaY);
    // PW_JUSTIFIED_SLEEP: stimulus — input pacing IS the zoom gesture being measured.
    await page.waitForTimeout(16);
    i++;
  }
}

export async function measureCanvasBaseline(page: Page, fixtureNodes: number): Promise<CanvasBaselineSample> {
  const mounted = await loadPerfCanvas(page, fixtureNodes);
  if (!mounted) throw new Error(`canvas perf fixture ${fixtureNodes} did not mount`);

  await panToMountTaskCards(page, 1.25);
  await drivePan(page, 450);
  await driveZoom(page, 450);
  await waitForCanvasSettled(page);

  await startSampler(page);
  // PW_JUSTIFIED_SLEEP: probe — the window IS the idle measurement, not a wait.
  await page.waitForTimeout(1200);
  const idle = await stopSampler(page);

  await startSampler(page);
  await drivePan(page, 1600);
  await waitForCanvasSettled(page);
  const pan = await stopSampler(page);

  await setCanvasScale(page, 1.35);
  await startSampler(page);
  await driveZoom(page, 1600);
  await waitForCanvasSettled(page);
  const zoom = await stopSampler(page);

  return {
    fixtureNodes,
    scale: round1(await canvasScale(page)),
    worldCanvasCount: await page.locator('[data-testid="canvas-world-canvas"]').count(),
    worldCardDomCount: await page.locator('[data-testid="canvas-task-card"]').count(),
    presencePinCount: await page.locator('[data-testid="canvas-presence-pin"]').count(),
    presenceOverlayCount: await page.locator('[data-testid="canvas-presence-overlay"]').count(),
    swarmMemberCount: await page.locator('[data-testid="canvas-swarm-member"], [data-testid="canvas-swarm-face"]').count(),
    liveAgentTextCount: await readLiveAgentTextCount(page),
    idle,
    pan,
    zoom,
  };
}

async function readLiveAgentTextCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const text = document.body.innerText;
    const match = text.match(/\b(\d+)\s+working\b/);
    return match ? Number(match[1]) : 0;
  });
}

export function l5Thresholds(samples: CanvasBaselineSample[]): CanvasBaselineThreshold[] {
  return samples.map((s) => ({
    fixtureNodes: s.fixtureNodes,
    panP95Ms: thresholdFromP95(s.pan.p95),
    zoomP95Ms: thresholdFromP95(s.zoom.p95),
    longtaskCount: Math.max(0, Math.floor(Math.max(s.pan.longtaskCount, s.zoom.longtaskCount) * 0.75)),
  }));
}

export function formatBaselineTable(samples: CanvasBaselineSample[]): string {
  const rows = [
    "nodes | cards | pins | swarm | live text | idle p95 | pan p95 | zoom p95 | pan paints | zoom paints | pan overlay builds | zoom overlay builds | pan commits | zoom commits | pan blits | zoom blits | pan longtasks | zoom longtasks",
    "---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---:",
    ...samples.map((s) =>
      [
        s.fixtureNodes,
        s.worldCardDomCount,
        s.presencePinCount,
        s.swarmMemberCount,
        s.liveAgentTextCount,
        `${s.idle.p95}ms`,
        `${s.pan.p95}ms`,
        `${s.zoom.p95}ms`,
        s.pan.churn.cameraPaints,
        s.zoom.churn.cameraPaints,
        s.pan.churn.presenceOverlayBuilds,
        s.zoom.churn.presenceOverlayBuilds,
        s.pan.churn.canvasCommits,
        s.zoom.churn.canvasCommits,
        s.pan.churn.canvasBlits,
        s.zoom.churn.canvasBlits,
        s.pan.longtaskCount,
        s.zoom.longtaskCount,
      ].join(" | "),
    ),
  ];
  return rows.join("\n");
}

export function formatThresholdTable(thresholds: CanvasBaselineThreshold[]): string {
  const rows = [
    "nodes | L5 pan p95 target | L5 zoom p95 target | L5 longtask target",
    "---: | ---: | ---: | ---:",
    ...thresholds.map((t) => `${t.fixtureNodes} | <=${t.panP95Ms}ms | <=${t.zoomP95Ms}ms | <=${t.longtaskCount}`),
  ];
  return rows.join("\n");
}
