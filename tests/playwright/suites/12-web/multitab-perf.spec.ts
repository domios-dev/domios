/**
 * WHAT: multitab perf covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { recordPerfMetric, recordPerfSamples } from "../../helpers/perf-metrics";
import { expect, test } from "../../helpers/fixtures";

// Cold app mount is out-of-band setup, not the measured churn window; bound it
// so a stuck mount fails fast under the default per-test timeout instead of
// consuming a 90s escalated budget.
const WEB_MOUNT_CAP_MS = 20_000;
const HIDDEN_PANE_RENDER_BUDGET = 3;

type RenderCounts = Record<string, number>;

async function readRenderCounts(page: import("@playwright/test").Page): Promise<RenderCounts> {
  return page.evaluate(() => ({ ...(window.__renderCounts ?? {}) }));
}

async function waitForRenderCounter(page: import("@playwright/test").Page): Promise<void> {
  await page.waitForFunction(
    () => typeof window.__muxPerfChurnPane === "function",
    undefined,
    { timeout: 30_000 },
  );
}

test("@perf hidden panes do not re-render on unrelated agent_state churn", async ({ ui, api, paneCleanup }) => {
  const { page } = ui;
  const suffix = Date.now();

  const paneA = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-perf-a-${suffix}`,
  });
  await api.post(`/panes/${paneA.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${paneA.id}/chat-view`, { enabled: true });

  const hiddenPanes: Array<{ id: string; tab_id?: string }> = [];
  for (let i = 0; i < 3; i += 1) {
    const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
      title: `pw-perf-hidden-${i}-${suffix}`,
    });
    await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
    hiddenPanes.push(pane);
  }

  const paneB = hiddenPanes[0]!;
  await api.focusPane(paneB.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: WEB_MOUNT_CAP_MS });
  await waitForRenderCounter(page);

  if (paneB.tab_id) {
    const tabB = page.locator(`[data-testid="tab-item"][data-tab-id="${paneB.tab_id}"]`).first();
    await expect(tabB).toBeVisible({ timeout: 30_000 });
    await tabB.click();
  }

  const visibleChat = page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneB.id}"]`);
  await expect(visibleChat).toBeVisible({ timeout: 30_000 });

  for (const hidden of hiddenPanes.slice(1)) {
    if (!hidden.tab_id) continue;
    const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${hidden.tab_id}"]`).first();
    await expect(tab).toBeAttached({ timeout: 30_000 });
  }

  await page.evaluate(() => {
    window.__renderCounts = {};
  });

  const before = await readRenderCounts(page);

  await page.evaluate(
    async ({ churnPaneId }) => {
      const churn = window.__muxPerfChurnPane;
      if (!churn) throw new Error("__muxPerfChurnPane test hook missing");
      const end = Date.now() + 3_000;
      while (Date.now() < end) {
        churn(churnPaneId);
        await new Promise((r) => setTimeout(r, 50)); // churn pacing: fixed-rate load generation
      }
    },
    { churnPaneId: paneA.id },
  );

  const after = await readRenderCounts(page);

  const hiddenIds = hiddenPanes.slice(1).map((p) => p.id);
  const hiddenRenderDeltas = hiddenIds.map((id) => ({
    id,
    delta: (after[id] ?? 0) - (before[id] ?? 0),
  }));
  recordPerfSamples({
    name: "hidden-pane-render-delta",
    unit: "count",
    samples: hiddenRenderDeltas.map(({ delta }) => delta),
    budget: HIDDEN_PANE_RENDER_BUDGET,
    mode: "stub",
  });
  for (const { id, delta } of hiddenRenderDeltas) {
    expect(
      delta,
      `hidden pane ${id} render delta during churn (pre-fix baseline ~60 over 3s)`,
    ).toBeLessThanOrEqual(HIDDEN_PANE_RENDER_BUDGET);
  }

  // The churn owner is itself hidden here, so the hidden-pane optimization
  // rightly suppresses its re-renders too; the signal must not be LOST —
  // surfacing the pane replays the churned state.
  if (paneA.tab_id) {
    const tabA = page.locator(`[data-testid="tab-item"][data-tab-id="${paneA.tab_id}"]`).first();
    await expect(tabA).toBeVisible({ timeout: 30_000 });
    await tabA.click();
  }
  await expect
    .poll(async () => (await readRenderCounts(page))[paneA.id] ?? 0, { timeout: 15_000 })
    .toBeGreaterThan(before[paneA.id] ?? 0);

  expect(Object.keys(after).length, "__renderCounts must have fired for at least one pane").toBeGreaterThan(0);
});

test("@perf hidden panes longtask budget during agent_state churn", async ({ ui, api, paneCleanup }) => {
  const { page } = ui;
  const suffix = Date.now();

  const paneA = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-perf-lt-a-${suffix}`,
  });
  await api.post(`/panes/${paneA.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${paneA.id}/chat-view`, { enabled: true });

  const paneB = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-perf-lt-b-${suffix}`,
  });
  await api.post(`/panes/${paneB.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${paneB.id}/chat-view`, { enabled: true });

  await api.focusPane(paneB.id);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: WEB_MOUNT_CAP_MS });
  await waitForRenderCounter(page);

  if (paneB.tab_id) {
    await page.locator(`[data-testid="tab-item"][data-tab-id="${paneB.tab_id}"]`).first().click();
  }

  await page.evaluate(() => {
    const w = window as unknown as { __longtaskMs?: number; __longtaskObs?: PerformanceObserver };
    w.__longtaskMs = 0;
    const po = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        w.__longtaskMs = (w.__longtaskMs ?? 0) + e.duration;
      }
    });
    po.observe({ type: "longtask", buffered: false } as PerformanceObserverInit);
    w.__longtaskObs = po;
  });

  await page.evaluate(
    async ({ churnPaneId }) => {
      const churn = window.__muxPerfChurnPane;
      if (!churn) throw new Error("__muxPerfChurnPane test hook missing");
      const end = Date.now() + 3_000;
      while (Date.now() < end) {
        churn(churnPaneId);
        await new Promise((r) => setTimeout(r, 50)); // churn pacing: fixed-rate load generation
      }
    },
    { churnPaneId: paneA.id },
  );

  const longtaskMs = await page.evaluate(() => (window as unknown as { __longtaskMs?: number }).__longtaskMs ?? 0);
  // Pre-fix baseline was well above this on a warm multi-tab session; post Stage-1 should stay low.
  recordPerfMetric({ name: "hidden-pane-longtask", unit: "ms", value: longtaskMs, budget: 500 });
  expect(longtaskMs).toBeLessThan(500);
});
