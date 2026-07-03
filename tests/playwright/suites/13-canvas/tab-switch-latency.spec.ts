/**
 * Tab-switch latency audit (task_fd79beb6, loop_f5854596).
 *
 * Measures click-to-next-paint latency for sidebar tab switches under a WARM
 * session: 10+ mounted terminal panes plus a live plan/canvas workspace
 * overlay. Uses the browser's native Event Timing API (the signal behind the
 * Interaction-to-Next-Paint web vital) — an interaction entry's `duration`
 * already spans input delay + processing + presentation (paint), so it is a
 * faithful "click to first paint" measurement, not a hand-rolled proxy.
 *
 * Run (dev on 7686):
 *   CT_E2E_PORT=7686 npx playwright test -c tests/playwright -g "tab-switch-latency" --workers=1
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { REPO_ROOT } from "../../helpers/dev-console";

const PANE_COUNT = 20; // 10+ per the audit's warm-session requirement, with headroom
const SWITCH_BUDGET_MS = 150;

interface InpSample {
  name: string;
  start: number;
  duration: number;
}

async function startInpSampler(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __inp?: InpSample[]; __inpObs?: PerformanceObserver };
    w.__inp = [];
    const po = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.name === "click") {
          w.__inp?.push({ name: e.name, start: e.startTime, duration: e.duration });
        }
      }
    });
    // 16ms is Chromium's minimum granularity for event-timing durationThreshold;
    // default (104ms) would silently drop everything under our 150ms budget.
    po.observe({ type: "event", durationThreshold: 16, buffered: false } as PerformanceObserverInit);
    w.__inpObs = po;
  });
}

async function collectInp(page: Page): Promise<InpSample[]> {
  return page.evaluate(() => (window as unknown as { __inp?: InpSample[] }).__inp ?? []);
}

async function resetInp(page: Page): Promise<void> {
  await page.evaluate(() => {
    (window as unknown as { __inp?: InpSample[] }).__inp = [];
  });
}

interface Stats {
  n: number;
  p50: number;
  p95: number;
  max: number;
}

function stats(samples: InpSample[]): Stats {
  const d = samples.map((s) => s.duration).sort((a, b) => a - b);
  const q = (p: number) => (d.length ? d[Math.min(d.length - 1, Math.floor((p / 100) * d.length))] : 0);
  return { n: d.length, p50: q(50), p95: q(95), max: d[d.length - 1] ?? 0 };
}

function fmt(label: string, s: Stats): string {
  return `${label.padEnd(14)} n=${String(s.n).padStart(3)}  p50=${String(s.p50).padStart(5)}ms  p95=${String(s.p95).padStart(5)}ms  max=${String(s.max).padStart(5)}ms`;
}

test("@tab-switch-latency sidebar tab switch reaches paint under budget", async ({ ui, api, paneCleanup }) => {
  test.setTimeout(180_000);
  const { page } = ui;

  const cwd = path.resolve(REPO_ROOT);
  const ws = await api.post<{ id: string }>("/workspaces", {
    name: "tab-switch-perf",
    cwd,
    activate: true,
  });
  const wsId = ws.id;

  const plan = await api.post<{ plan_id: string }>("/plans", {
    title: "tab-switch-perf plan",
    project_path: cwd,
  });

  const tabs: { tab_id: string; pane_id: string }[] = [];
  try {
    for (let i = 0; i < PANE_COUNT; i++) {
      const t = await api.post<{ tab_id: string; pane_id: string }>(`/workspaces/${wsId}/tabs`, { eager: true });
      tabs.push(t);
      paneCleanup.track(t.pane_id);
    }

    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#root > *", { state: "attached", timeout: 60_000 });
    await page.waitForSelector(`[data-testid="tab-item"][data-tab-id="${tabs[0].tab_id}"]`, { timeout: 30_000 });

    // Warm: visit every tab once so it enters mountedTabIds — the realistic
    // "already open" state a real warm session would be in, not a cold mount.
    for (const t of tabs) {
      await page.click(`[data-testid="tab-item"][data-tab-id="${t.tab_id}"]`);
      await page.waitForTimeout(80);
    }
    await page.waitForTimeout(300);

    // Touch the canvas overlay once so its lazy data (plans/panes) is fetched
    // before the timed windows — the timed runs measure render/mount cost,
    // not the one-time network fetch.
    await page.click('[data-testid="canvas-toolbar-button"]');
    await page.waitForSelector('[data-testid="mission-control-plan-surface"]', { state: "attached", timeout: 15_000 });
    await page.click('[data-testid="canvas-toolbar-button"]');
    await page.waitForTimeout(300);

    // ---- Timed window 1: round-robin terminal tab switches ----
    await startInpSampler(page);
    const N = 24;
    for (let i = 0; i < N; i++) {
      const t = tabs[i % tabs.length];
      await page.click(`[data-testid="tab-item"][data-tab-id="${t.tab_id}"]`);
      await page.waitForTimeout(120);
    }
    await page.waitForTimeout(300);
    const tabSamples = await collectInp(page);

    // ---- Timed window 2: canvas overlay open/close (the "plan/canvas" surface) ----
    await resetInp(page);
    const M = 12;
    for (let i = 0; i < M; i++) {
      await page.click('[data-testid="canvas-toolbar-button"]');
      await page.waitForTimeout(150);
    }
    await page.waitForTimeout(300);
    const canvasSamples = await collectInp(page);

    const ipc = await page.evaluate(() =>
      ((window as unknown as { __ctPerf?: { cmd: string; ms: number }[] }).__ctPerf ?? []).map(
        (e) => `${e.cmd}:${Math.round(e.ms)}ms`,
      ),
    );

    const tabStats = stats(tabSamples);
    const canvasStats = stats(canvasSamples);

    // eslint-disable-next-line no-console
    console.log(
      `\n──────── tab-switch-latency (dev build, panes=${PANE_COUNT}) ────────\n` +
        `${fmt("tab-switch", tabStats)}\n${fmt("canvas-toggle", canvasStats)}\n` +
        `IPC/LONGTASK: ${ipc.length ? ipc.join(", ") : "none"}\n` +
        `─────────────────────────────────────────────────────────────\n`,
    );

    expect(tabStats.n, "captured tab-switch interaction samples").toBeGreaterThan(5);
    expect(tabStats.p95, `tab-switch p95 ${tabStats.p95}ms over budget ${SWITCH_BUDGET_MS}ms`).toBeLessThanOrEqual(
      SWITCH_BUDGET_MS,
    );
  } finally {
    await api.request("DELETE", `/plans/${plan.plan_id}`).catch(() => {});
  }
});
