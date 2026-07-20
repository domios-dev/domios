/**
 * WHAT: tab switch latency covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { recordPerfMetric } from "../../helpers/perf-metrics";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import type { Page } from "@playwright/test";
import { REPO_ROOT } from "../../helpers/dev-console";

const PANE_COUNT = 20; // 10+ per the audit's warm-session requirement, with headroom
// Fixed waits here are measurement pacing windows of this latency harness.
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

test("@perf @tab-switch-latency sidebar tab switch reaches paint under budget", async ({ ui, api, paneCleanup }) => {
  const { page } = ui;

  const cwd = path.resolve(REPO_ROOT);
  const ws = await api.post<{ id: string }>("/workspaces", {
    name: "tab-switch-perf",
    cwd,
    activate: true,
  });
  const wsId = ws.id;

  const tabs: { tab_id: string; pane_id: string }[] = [];
  let plan: { plan_id: string } | undefined;
  try {
    for (let i = 0; i < PANE_COUNT; i++) {
      const t = await api.post<{ tab_id: string; pane_id: string }>(`/workspaces/${wsId}/tabs`, { eager: true });
      tabs.push(t);
      paneCleanup.track(t.pane_id);
    }
    await api.post(`/panes/${tabs[0].pane_id}/role`, { role: "Orchestrator" });
    plan = await api.post<{ plan_id: string }>("/plans", {
      title: "tab-switch-perf plan",
      project_path: cwd,
      owner: {
        pane_id: tabs[0].pane_id,
        session_id: null,
        title: "tab-switch-perf owner",
      },
    });

    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#root > *", { state: "attached", timeout: 60_000 });
    await page.waitForSelector(`[data-testid="tab-item"][data-tab-id="${tabs[0].tab_id}"]`, { timeout: 30_000 });

    // "already open" state a real warm session would be in, not a cold mount.
    for (const t of tabs) {
      await page.click(`[data-testid="tab-item"][data-tab-id="${t.tab_id}"]`);
      // PW_JUSTIFIED_SLEEP: fixed 80ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(80);
    }
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(300);

    await page.click('[data-testid="canvas-toolbar-button"]');
    await page.waitForSelector('[data-testid="mission-control-plan-surface"]', { state: "attached", timeout: 15_000 });
    await page.click('[data-testid="canvas-toolbar-button"]');
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(300);

    await startInpSampler(page);
    const N = 24;
    for (let i = 0; i < N; i++) {
      const t = tabs[i % tabs.length];
      await page.click(`[data-testid="tab-item"][data-tab-id="${t.tab_id}"]`);
      // PW_JUSTIFIED_SLEEP: fixed 120ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(120);
    }
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(300);
    const tabSamples = await collectInp(page);

    await resetInp(page);
    const M = 12;
    for (let i = 0; i < M; i++) {
      await page.click('[data-testid="canvas-toolbar-button"]');
      // PW_JUSTIFIED_SLEEP: fixed 150ms sampling/settle window; no app event exists for this probe.
      await page.waitForTimeout(150);
    }
    // PW_JUSTIFIED_SLEEP: fixed 300ms sampling/settle window; no app event exists for this probe.
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
    recordPerfMetric({
      name: "tab-switch-paint",
      unit: "ms",
      n: tabStats.n,
      p50: tabStats.p50,
      p95: tabStats.p95,
      max: tabStats.max,
      budget: SWITCH_BUDGET_MS,
    });
    expect(tabStats.p95, `tab-switch p95 ${tabStats.p95}ms over budget ${SWITCH_BUDGET_MS}ms`).toBeLessThanOrEqual(
      SWITCH_BUDGET_MS,
    );
  } finally {
    if (plan) await api.request("DELETE", `/plans/${plan.plan_id}`).catch(() => {});
  }
});
