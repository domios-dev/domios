/**
 * WHAT: Stub-provider performance gate for visible terminal interactions.
 * WHY: PTY output must not make replay, history, reload, or IPC churn feel frozen.
 * ORACLE: A rendered terminal marker and its bottom anchor, never daemon logs alone.
 */
import os from "node:os";
import { expect, test, stubEnvActive, STUB_ENV_ABSENT_SKIP, type Page } from "../../helpers/perf-fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { recordPerfSamples, recordPerfEvent } from "../../helpers/perf-metrics";
import { formatPerfFailureState, PerfEventBounce } from "../../helpers/perf-event-bounce";
import { pollUntil } from "../../helpers/poll";
import { showTerminalView } from "../../helpers/stub-gate";

const WRITE_RENDER_P95_BUDGET_MS = 2_000;
const WRITE_RENDER_MAX_BUDGET_MS = 3_000;
const REPLAY_LONGTASK_BUDGET_MS = 250;
const COLD_INTERACTIVE_BUDGET_MS = 15_000;
const CONFIG_P95_BUDGET_MS = 150;
const SAMPLE_COUNT = 5;
const READY_CAP_MS = 8_000;
const SAMPLE_WAIT_SLACK_MS = 1_500;

interface FocusViewState {
  rootMounted: boolean;
  paneVisible: boolean;
  tabActive: boolean;
  terminalMode: boolean;
}

interface ViewportGeometry {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  bottomDelta: number;
  anchored: boolean;
}

function computeAnchored(scrollTop: number, scrollHeight: number, clientHeight: number): Pick<ViewportGeometry, "bottomDelta" | "anchored"> {
  const bottomDelta = scrollHeight - clientHeight - scrollTop;
  return { bottomDelta, anchored: scrollHeight <= clientHeight || bottomDelta <= 2 };
}

test.beforeEach(({ perfSeed }) => {
  // Env present + inactive seed = false-active; FAIL loud (never skip hard metrics).
  if (stubEnvActive() && !perfSeed.active) throw new Error("CODETERM_STUB_AGENT_CMD present but perf seed inactive; hard metrics must not skip");
  test.skip(!perfSeed.active, STUB_ENV_ABSENT_SKIP);
});

async function markerVisible(page: Page, paneId: string, marker: string): Promise<boolean> {
  return page.evaluate(
    ({ id, needle }) => {
      const pane = document.querySelector(`[data-pane-id="${id}"]`);
      return Array.from(pane?.querySelectorAll(".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows") ?? [])
        .some((root) => ((root as HTMLElement).innerText || root.textContent || "").includes(needle));
    },
    { id: paneId, needle: marker },
  );
}

async function focusTerminal(page: Page, api: ApiClient, paneId: string): Promise<void> {
  const pane = (await api.panes()).find((entry) => entry.id === paneId) as
    | { tab_id?: string; workspace_id?: string; title?: string }
    | undefined;
  if (pane?.workspace_id) await api.focusWorkspace(pane.workspace_id);
  if (pane?.tab_id) {
    const tab = pane.title
      ? page.getByRole("button", { name: `Open tab ${pane.title}`, exact: true }).first()
      : page.locator(`[data-tab-id="${pane.tab_id}"]`).first();
    await tab.scrollIntoViewIfNeeded().catch(() => {});
    if (await tab.isVisible().catch(() => false)) {
      await tab.click();
      await expect(tab).toHaveAttribute("aria-current", "page", { timeout: READY_CAP_MS });
    }
    else {
      await api.focusPane(paneId);
      await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
    }
  } else {
    await api.focusPane(paneId);
    await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
  }
  await expect(page.locator(`[data-pane-id="${paneId}"]`).first()).toBeVisible({ timeout: READY_CAP_MS });
  await showTerminalView(page, paneId);
}

async function paneText(api: ApiClient, paneId: string): Promise<string> {
  const res = await api.get<{ text?: string } | string>(`/panes/${paneId}/text`);
  return typeof res === "string" ? res : (res.text ?? "");
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function readFocusView(page: Page, api: ApiClient, paneId: string): Promise<FocusViewState> {
  const focused = (await api.panes()).find((p) => (p as { id?: string }).id === paneId) as { focused?: boolean; tab_id?: string } | undefined;
  const dom = await page.evaluate((id) => {
    const root = document.querySelector("#root");
    const pane = document.querySelector(`[data-pane-id="${id}"]`) as HTMLElement | null;
    const term = pane?.querySelector('[data-testid="pane-mode-switch"] [role="tab"][aria-selected="true"]');
    return {
      rootMounted: Boolean(root && root.childElementCount > 0),
      paneVisible: Boolean(pane && pane.offsetParent !== null),
      terminalMode: (term?.textContent ?? "").includes("Terminal"),
    };
  }, paneId);
  return { ...dom, tabActive: Boolean(focused?.focused) };
}

async function readViewport(page: Page, paneId: string): Promise<ViewportGeometry | null> {
  const raw = await page.evaluate((id) => {
    const vp = document.querySelector(`[data-pane-id="${id}"] .xterm-viewport`) as HTMLElement | null;
    if (!vp) return null;
    return { scrollTop: vp.scrollTop, scrollHeight: vp.scrollHeight, clientHeight: vp.clientHeight };
  }, paneId);
  if (!raw) return null;
  const { bottomDelta, anchored } = computeAnchored(raw.scrollTop, raw.scrollHeight, raw.clientHeight);
  return { ...raw, bottomDelta, anchored };
}

// Polls pane text and DOM in the SAME pollUntil budget/cadence as before; the
// render oracle stays the exit condition, output timing is a side-channel probe.
async function sendAndMeasureRender(page: Page, api: ApiClient, paneId: string, marker: string, name: string, index: number): Promise<number> {
  const budgetMs = WRITE_RENDER_MAX_BUDGET_MS + SAMPLE_WAIT_SLACK_MS;
  const startedAt = performance.now();
  const bounce = new PerfEventBounce(
    { run: process.env.PERF_DIAG_RUN ?? `perf-${process.pid}-${test.info().workerIndex}`, test: name, index, startedAt },
    recordPerfEvent,
  );
  await api.post(`/panes/${paneId}/send`, { text: `STUB_ECHO ${marker}\r`, raw: true });
  bounce.record("sent", { marker, paneId, loadavg: os.loadavg() });
  let tOutputMs: number | null = null;
  let tRenderMs: number | null = null;
  await pollUntil(
    async () => {
      if (tOutputMs === null) {
        try {
          if ((await paneText(api, paneId)).includes(marker)) {
            tOutputMs = performance.now() - startedAt;
            bounce.record("output", { marker, loadavg: os.loadavg() });
          }
        } catch (error) {
          bounce.record("output-error", { error: errorText(error), loadavg: os.loadavg() });
        }
      }
      if (await markerVisible(page, paneId, marker)) {
        tRenderMs = performance.now() - startedAt;
        const [focus, viewport] = await Promise.all([readFocusView(page, api, paneId), readViewport(page, paneId)]);
        bounce.record("render", { marker, loadavg: os.loadavg(), focus, viewport }, tRenderMs);
        return true;
      }
      return false;
    },
    budgetMs,
    50,
    `visible marker ${marker}`,
  ).catch(() => undefined);
  if (tRenderMs === null) await captureFailureDiagnostics(page, api, paneId, marker, name, index, budgetMs, bounce);
  return tRenderMs ?? performance.now() - startedAt;
}

// Bounded single diagnostic pass on render timeout: one text fetch, status, DOM
// re-poll, screenshot; throws a phase-named message. No loops/sleeps/retries.
async function captureFailureDiagnostics(page: Page, api: ApiClient, paneId: string, marker: string, name: string, index: number, budgetMs: number, bounce: PerfEventBounce): Promise<never> {
  let finalText = "";
  let outputError: string | null = null;
  try {
    finalText = await paneText(api, paneId);
    if (finalText.includes(marker) && !bounce.recorded("output")) {
      const tOutputMs = bounce.elapsedMs();
      bounce.record("output", { marker, late: true, loadavg: os.loadavg() }, tOutputMs);
    }
  } catch (error) {
    outputError = errorText(error);
    bounce.record("output-error", { error: outputError, late: true, loadavg: os.loadavg() });
  }
  const status = await api.get(`/panes/${paneId}/status`).catch(() => null);
  const finalFocus = await readFocusView(page, api, paneId);
  const finalViewport = await readViewport(page, paneId);
  const rerender = await markerVisible(page, paneId, marker).catch(() => false);
  if (rerender && !bounce.recorded("render")) {
    const tRenderMs = bounce.elapsedMs();
    bounce.record("render", { marker, late: true, loadavg: os.loadavg(), focus: finalFocus, viewport: finalViewport }, tRenderMs);
  }
  const outputPath = test.info().outputPath(`perf-diag-${name}-${index}-${Date.now()}.png`);
  await page.screenshot({ path: outputPath }).catch(() => {});
  bounce.record("capture", { outputPath, finalTextHasMarker: finalText.includes(marker), rerenderVisible: rerender, loadavg: os.loadavg(), focus: finalFocus, viewport: finalViewport, status, ...(outputError === null ? {} : { outputError }) });
  throw new Error(formatPerfFailureState(bounce, budgetMs));
}

async function installLongtaskObserver(page: Page): Promise<void> {
  await page.evaluate(() => {
    const target = window as typeof window & { perfGateLongtasks?: number; perfGateObserver?: PerformanceObserver };
    target.perfGateLongtasks = 0;
    target.perfGateObserver?.disconnect();
    const observer = new PerformanceObserver((entries) => {
      for (const entry of entries.getEntries()) target.perfGateLongtasks = (target.perfGateLongtasks ?? 0) + entry.duration;
    });
    observer.observe({ entryTypes: ["longtask"] });
    target.perfGateObserver = observer;
  });
}

async function longtaskMs(page: Page): Promise<number> {
  return page.evaluate(() => (window as typeof window & { perfGateLongtasks?: number }).perfGateLongtasks ?? 0);
}

async function bottomAnchored(page: Page, paneId: string): Promise<boolean> {
  return page.evaluate((id) => {
    const viewport = document.querySelector(`[data-pane-id="${id}"] .xterm-viewport`) as HTMLElement | null;
    if (!viewport) return false;
    return viewport.scrollHeight <= viewport.clientHeight || viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= 2;
  }, paneId);
}

test("@perf stub PTY write reaches rendered terminal within budget", async ({ ui, perfSeed }) => {
  const { page } = ui;
  const { api } = perfSeed;
  const { front } = perfSeed.write;
  await focusTerminal(page, api, front);

  const samples: number[] = [];
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    samples.push(await sendAndMeasureRender(page, api, front, `PWRENDER${Date.now()}${index}`, "pty-write-to-render", index));
  }
  const summary = recordPerfSamples({
    name: "pty-write-to-render",
    unit: "ms",
    samples,
    budget: WRITE_RENDER_P95_BUDGET_MS,
    mode: "stub",
  });
  expect(summary.p95, `PTY write-to-render p95 ${summary.p95}ms`).toBeLessThanOrEqual(WRITE_RENDER_P95_BUDGET_MS);
  expect(summary.max, `PTY write-to-render max ${summary.max}ms`).toBeLessThanOrEqual(WRITE_RENDER_MAX_BUDGET_MS);
});

test("@perf stub long history opens bottom-anchored without a long task", async ({ ui, perfSeed }) => {
  const { page } = ui;
  const { api } = perfSeed;
  const { front, history, tail } = perfSeed.history;
  await focusTerminal(page, api, front);
  await installLongtaskObserver(page);

  const samples: number[] = [];
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    await focusTerminal(page, api, front);
    const startedAt = performance.now();
    await focusTerminal(page, api, history);
    await pollUntil(() => markerVisible(page, history, tail), WRITE_RENDER_MAX_BUDGET_MS + SAMPLE_WAIT_SLACK_MS, 50, "history tail is visible");
    samples.push(performance.now() - startedAt);
    expect(await bottomAnchored(page, history), "long history viewport stays at the bottom").toBe(true);
  }
  const surface = recordPerfSamples({
    name: "long-history-bottom-anchor",
    unit: "ms",
    samples,
    budget: WRITE_RENDER_P95_BUDGET_MS,
    mode: "stub",
  });
  const jank = await longtaskMs(page);
  recordPerfSamples({ name: "long-history-open-longtask", unit: "ms", samples: [jank], budget: REPLAY_LONGTASK_BUDGET_MS, mode: "stub" });
  expect(surface.p95, `long-history bottom-anchor p95 ${surface.p95}ms`).toBeLessThanOrEqual(WRITE_RENDER_P95_BUDGET_MS);
  expect(jank, `long-history longtask total ${jank}ms`).toBeLessThan(REPLAY_LONGTASK_BUDGET_MS);
});

test("@perf stub cold reload reaches an interactive terminal within budget", async ({ ui, perfSeed }) => {
  const { page } = ui;
  const { api } = perfSeed;
  const { front } = perfSeed.cold;
  await focusTerminal(page, api, front);

  const startedAt = performance.now();
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.locator(`[data-pane-id="${front}"] .xterm`).first()).toBeVisible({
    timeout: COLD_INTERACTIVE_BUDGET_MS + SAMPLE_WAIT_SLACK_MS,
  });
  await pollUntil(
    () => markerVisible(page, front, "stub replay"),
    COLD_INTERACTIVE_BUDGET_MS + SAMPLE_WAIT_SLACK_MS,
    50,
    "stub terminal is interactive after reload",
  );
  const coldMs = performance.now() - startedAt;
  const summary = recordPerfSamples({
    name: "cold-interactive",
    unit: "ms",
    samples: [coldMs],
    budget: COLD_INTERACTIVE_BUDGET_MS,
    mode: "stub",
  });
  expect(summary.max, `cold interactive reload ${summary.max}ms`).toBeLessThanOrEqual(COLD_INTERACTIVE_BUDGET_MS);
});

test("@perf stub IPC stays visibly responsive during output churn", async ({ ui, perfSeed }) => {
  const { page } = ui;
  const { api } = perfSeed;
  const { front } = perfSeed.churn;
  await focusTerminal(page, api, front);

  const configSamples: number[] = [];
  const visibleSamples: number[] = [];
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    const configStartedAt = performance.now();
    await api.get("/config");
    configSamples.push(performance.now() - configStartedAt);
    visibleSamples.push(await sendAndMeasureRender(page, api, front, `PWIPC${Date.now()}${index}`, "ipc-starvation-visible-render", index));
  }
  const config = recordPerfSamples({
    name: "config-latency-under-churn",
    unit: "ms",
    samples: configSamples,
    budget: CONFIG_P95_BUDGET_MS,
    mode: "stub",
  });
  const visible = recordPerfSamples({
    name: "ipc-starvation-visible-render",
    unit: "ms",
    samples: visibleSamples,
    budget: WRITE_RENDER_P95_BUDGET_MS,
    mode: "stub",
  });
  expect(config.p95, `config IPC p95 ${config.p95}ms under churn`).toBeLessThanOrEqual(CONFIG_P95_BUDGET_MS);
  expect(visible.p95, `visible PTY render p95 ${visible.p95}ms under churn`).toBeLessThanOrEqual(WRITE_RENDER_P95_BUDGET_MS);
  expect(visible.max, `visible PTY render max ${visible.max}ms under churn`).toBeLessThanOrEqual(WRITE_RENDER_MAX_BUDGET_MS);
});
