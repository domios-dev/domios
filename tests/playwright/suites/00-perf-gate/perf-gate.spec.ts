/**
 * WHAT: perf gate — the four latency invariants the promoted budget tests
 *       do not cover: PTY history replay on surface, long-history bottom
 *       anchoring, cold open to interactive, and daemon responsiveness
 *       under output churn.
 * WHY: these are the user-facing freeze classes from the 1.7.2 stability
 *      effort; each budget is a hard assert and every run records the
 *      measured stats to artifacts/perf/metrics.jsonl.
 * ORACLE: rendered xterm state and wall-clock latencies read from the live
 *      app — never the test's own bookkeeping.
 */
import { expect, test } from "../../helpers/fixtures";
import { injectCast, CAST } from "../../helpers/inject-cast";
import { recordPerfMetric } from "../../helpers/perf-metrics";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";
import type { Page } from "@playwright/test";

const REPLAY_SURFACE_BUDGET_MS = 2_000;
const REPLAY_LONGTASK_BUDGET_MS = 250;
const COLD_OPEN_BUDGET_MS = 15_000;
const CONFIG_P95_BUDGET_MS = 150;

function stats(samples: number[]): { n: number; p50: number; p95: number; max: number } {
  const d = [...samples].sort((a, b) => a - b);
  const q = (p: number) => d[Math.min(d.length - 1, Math.ceil((p / 100) * d.length) - 1)] ?? 0;
  return { n: d.length, p50: q(50), p95: q(95), max: d[d.length - 1] ?? 0 };
}

async function installLongtaskObserver(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __perfGateLongtaskMs?: number; __perfGateObs?: PerformanceObserver };
    w.__perfGateLongtaskMs = 0;
    w.__perfGateObs?.disconnect();
    const obs = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        w.__perfGateLongtaskMs = (w.__perfGateLongtaskMs ?? 0) + e.duration;
      }
    });
    obs.observe({ entryTypes: ["longtask"] });
    w.__perfGateObs = obs;
  });
}

async function longtaskMs(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as unknown as { __perfGateLongtaskMs?: number }).__perfGateLongtaskMs ?? 0,
  );
}

async function paneMarkerVisible(page: Page, paneId: string, marker: string): Promise<boolean> {
  return page.evaluate(
    ({ paneId: id, marker: needle }) => {
      const pane = document.querySelector(`[data-pane-id="${id}"]`);
      if (!pane) return false;
      const roots = Array.from(
        pane.querySelectorAll(".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows"),
      );
      for (const el of roots) {
        if (((el as HTMLElement).innerText || el.textContent || "").includes(needle)) return true;
      }
      return false;
    },
    { paneId, marker },
  );
}

async function awaitPtyReady(api: ApiClient, paneId: string): Promise<void> {
  // A fresh pane's PTY materializes asynchronously after create.
  await pollUntil(
    async () => {
      try {
        await api.get(`/panes/${paneId}/output`);
        return true;
      } catch {
        return false;
      }
    },
    30_000,
    250,
    `pane ${paneId} PTY ready`,
  );
}

async function focusTab(page: Page, api: ApiClient, paneId: string): Promise<void> {
  // Desktop unmounts inactive tabs, and REST pane focus alone does not
  // activate a tab there — click the tab when reachable, fall back to REST
  // focus plus a state refresh when the sidebar has it scrolled out.
  const pane = (await api.panes()).find((p) => p.id === paneId) as
    | { tab_id?: string; workspace_id?: string }
    | undefined;
  if (pane?.workspace_id) await api.focusWorkspace(pane.workspace_id);
  if (pane?.tab_id) {
    const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).first();
    await tab.scrollIntoViewIfNeeded().catch(() => {});
    if (await tab.isVisible().catch(() => false)) {
      await tab.click();
    } else {
      await api.focusPane(paneId);
      await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
    }
  } else {
    await api.focusPane(paneId);
  }
  await expect(page.locator(`[data-pane-id="${paneId}"]`).first()).toBeVisible({ timeout: 30_000 });
}

test("@perf hidden-pane PTY replay surfaces within budget and frame-bounded", async ({
  ui,
  api,
  paneCleanup,
}) => {
  test.setTimeout(180_000);
  await ui.reconnect();
  const { page } = ui;
  const front = await paneCleanup.createPane({ title: "pw-replay-front", eager: true });
  const back = await paneCleanup.createPane({ title: "pw-replay-back", eager: true });
  await focusTab(page, api, front.id);

  // Load real recorded output into the HIDDEN pane, then a trailing marker.
  const marker = `PWREPLAY${Date.now()}`;
  await awaitPtyReady(api, back.id);
  await injectCast(api, { paneId: back.id, castPath: CAST.claudeSystemsDesignLongChatIdle, instant: true });
  await api.post(`/panes/${back.id}/send`, { text: `echo ${marker}`, enter: true });
  await pollUntil(
    async () => {
      const t = await api.get<{ text?: string } | string>(`/panes/${back.id}/text`).catch(() => "");
      return String(typeof t === "string" ? t : (t.text ?? "")).includes(marker);
    },
    30_000,
    250,
    "marker lands in the hidden pane buffer",
  );

  await installLongtaskObserver(page);
  const t0 = Date.now();
  await focusTab(page, api, back.id);
  await pollUntil(
    () => paneMarkerVisible(page, back.id, marker),
    REPLAY_SURFACE_BUDGET_MS + 10_000,
    100,
    "replayed history reaches the surfaced pane's DOM",
  );
  const surfaceMs = Date.now() - t0;
  const jank = await longtaskMs(page);

  recordPerfMetric({ name: "pty-replay-surface", unit: "ms", value: surfaceMs, budget: REPLAY_SURFACE_BUDGET_MS });
  recordPerfMetric({ name: "pty-replay-longtask", unit: "ms", value: jank, budget: REPLAY_LONGTASK_BUDGET_MS });
  expect(surfaceMs, `surfacing replay took ${surfaceMs}ms`).toBeLessThanOrEqual(REPLAY_SURFACE_BUDGET_MS);
  expect(jank, `replay longtask total ${jank}ms`).toBeLessThan(REPLAY_LONGTASK_BUDGET_MS);
});

test("@perf long history opens anchored at the bottom without burst scroll", async ({
  ui,
  api,
  paneCleanup,
}) => {
  test.setTimeout(180_000);
  await ui.reconnect();
  const { page } = ui;
  const front = await paneCleanup.createPane({ title: "pw-anchor-front", eager: true });
  const pane = await paneCleanup.createPane({ title: "pw-anchor-long", eager: true });
  await focusTab(page, api, front.id);

  const tail = `PWANCHOR${Date.now()}`;
  await awaitPtyReady(api, pane.id);
  await injectCast(api, { paneId: pane.id, castPath: CAST.claudeSystemsDesignLongChatIdle, instant: true });
  await api.post(`/panes/${pane.id}/send`, { text: `echo ${tail}`, enter: true });
  await pollUntil(
    async () => {
      const t = await api.get<{ text?: string } | string>(`/panes/${pane.id}/text`).catch(() => "");
      return String(typeof t === "string" ? t : (t.text ?? "")).includes(tail);
    },
    30_000,
    250,
    "tail marker lands in the long-history buffer",
  );

  await installLongtaskObserver(page);
  await focusTab(page, api, pane.id);
  // Bottom anchor: the TAIL of the history is what the user sees, promptly.
  await pollUntil(
    () => paneMarkerVisible(page, pane.id, tail),
    12_000,
    100,
    "tail visible after opening long history",
  );
  const jank = await longtaskMs(page);
  const atBottom = await page.evaluate((paneId) => {
    const pane = document.querySelector(`[data-pane-id="${paneId}"]`);
    const vp = pane?.querySelector(".xterm-viewport") as HTMLElement | null;
    if (!vp) return true; // no scrollback viewport rendered = nothing scrolled
    return vp.scrollHeight - vp.scrollTop - vp.clientHeight < vp.clientHeight;
  }, pane.id);

  recordPerfMetric({ name: "long-history-open-longtask", unit: "ms", value: jank, budget: REPLAY_LONGTASK_BUDGET_MS });
  expect(atBottom, "viewport must land at (or within one screen of) the bottom").toBe(true);
  expect(jank, `open-at-bottom longtask total ${jank}ms`).toBeLessThan(REPLAY_LONGTASK_BUDGET_MS);
});

test("@perf cold open reaches an interactive shell within budget", async ({ ui }) => {
  test.setTimeout(120_000);
  const { page } = ui;
  const t0 = Date.now();
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: COLD_OPEN_BUDGET_MS + 30_000 });
  await page.waitForSelector('.xterm:visible, [data-testid="chat-composer"]:visible', {
    state: "visible",
    timeout: COLD_OPEN_BUDGET_MS + 30_000,
  });
  const coldMs = Date.now() - t0;
  recordPerfMetric({ name: "cold-open-interactive", unit: "ms", value: coldMs, budget: COLD_OPEN_BUDGET_MS });
  expect(coldMs, `cold open took ${coldMs}ms`).toBeLessThanOrEqual(COLD_OPEN_BUDGET_MS);
});

test("@perf daemon config endpoint stays responsive under output churn", async ({
  api,
  paneCleanup,
}) => {
  test.setTimeout(180_000);
  // Churn: a pane replaying a large recorded cast, paced, through the live
  // output pipeline while we sample the cheap config endpoint.
  const churn = await paneCleanup.createPane({ title: "pw-ipc-churn", eager: true });
  await awaitPtyReady(api, churn.id);
  await injectCast(api, {
    paneId: churn.id,
    castPath: CAST.claudeSystemsDesignLongChatIdle,
    speed: 8,
    classify: true,
  });

  const samples: number[] = [];
  const until = Date.now() + 8_000;
  while (Date.now() < until) {
    const t0 = performance.now();
    await api.get("/config");
    samples.push(performance.now() - t0);
  }
  const s = stats(samples);
  recordPerfMetric({ name: "config-latency-under-churn", unit: "ms", ...s, budget: CONFIG_P95_BUDGET_MS });
  expect(s.n, "collected latency samples during churn").toBeGreaterThan(30);
  expect(s.p95, `config p95 ${s.p95}ms under churn`).toBeLessThanOrEqual(CONFIG_P95_BUDGET_MS);
});
