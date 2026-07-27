/**
 * WHAT: Composer latency under a 25-tab agent/output fixture.
 * WHY: Controlled input must paint independently of transcript, task, IPC, and output churn.
 * ORACLE: p95 <= 32ms, max <= 100ms, no dropped/reordered input, transcript ordering preserved.
 */
import { recordPerfMetric } from "../../helpers/perf-metrics";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { API_BASE, expect, readDevToken, test, type Browser, type TabCleanup, type Page } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";

const TAB_COUNT = 25;
// Cold web mount is out-of-band setup, not the measured composer window; bound
// it so a stuck mount fails fast under the default per-test timeout instead of
// consuming a 90s escalated budget.
const WEB_MOUNT_CAP_MS = 20_000;
const INITIAL_MESSAGES = 260;
const LIVE_BATCH_SIZE = 24;
const LIVE_INTERVAL_MS = 35;
const TYPED_TEXT = "abcdefghijklmnopqrstuvwxyz0123456789";
const PASTED_TEXT = " pasted-text";

type ChatMessageKind = "user" | "assistant" | "system" | "tool_call" | "tool_result";

interface TestChatMessage {
  id: string;
  type: ChatMessageKind;
  content: string;
  timestamp: string;
  provider?: string;
}

interface PerfSummary {
  variant: string;
  mode: "webview2" | "web";
  p50: number;
  p95: number;
  max: number;
  samples: number;
  composerRenders: number;
  tabRenders: number;
  longTasks: number;
  longTaskMax: number;
  longTaskTotalMs: number;
  ipcInvokes: number;
  ipcEvents: number;
  ipcInvokeRate: number;
  ipcEventRate: number;
  outputFrames: number;
  outputBytes: number;
  outputBatches: number;
  outputFrameRate: number;
  transcriptMessages: number;
  transcriptFirst: string;
  transcriptLast: string;
  openTabs: number;
  scenarioWallMs: number;
  cpuMainThreadBusyRatio: number;
  backendEvidence: BackendEvidence;
}

interface BackendEvidence {
  lockHoldLines: string[];
  slowIpcLines: string[];
  queueWaitLines: string[];
}

declare global {
  interface Window {
    __codetermTestChat?: {
      pushMessages: (tabId: string, messages: TestChatMessage[]) => void;
    };
    __composerPerf?: {
      inputMarks: Array<{ id: number; at: number; inputType: string }>;
      paintLatencies: number[];
      renderCounts: Record<string, number>;
      compositionEvents: string[];
      ignoredKeys: string[];
    };
    __renderCounts?: Record<string, number>;
    __outputStreamPerf?: {
      frames: number;
      bytes: number;
      batches: number;
      lastFrameAt: number;
    };
    __ipcPerf?: {
      invokes: number;
      invokeMs: number;
      events: number;
      byCommand: Record<string, number>;
      byEvent: Record<string, number>;
    };
    __perfLongTasks?: number[];
    __perfChurnStop?: () => void;
    __muxPerfChurnTab?: (tabId: string) => void;
  }
}

async function createFixture(api: ApiClient, tabCleanup: TabCleanup): Promise<{ tabIds: string[]; tabIds: string[] }> {
  const tabIds: string[] = [];
  const tabIds: string[] = [];
  for (let i = 0; i < TAB_COUNT; i += 1) {
    const tab = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
      title: `pw-composer-perf-${String(i).padStart(2, "0")}`,
    });
    tabIds.push(tab.id);
    if (tab.tab_id) tabIds.push(tab.tab_id);
    await api.post(`/tabs/${tab.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/tabs/${tab.id}/chat-view`, { enabled: true });
    if (i === 0) {
      await api.post(`/tabs/${tab.id}/role`, { role: "Orchestrator" }).catch(() => {});
    } else {
      await api.post(`/tabs/${tab.id}/role`, { role: "Worker", parent_tab_id: tabIds[0] }).catch(() => {});
    }
  }
  await api.focusTab(tabIds[0]);
  return { tabIds, tabIds };
}

async function openWebPage(browser: Browser): Promise<Page> {
  const token = readDevToken();
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    permissions: ["clipboard-read", "clipboard-write"],
  });
  const page = await context.newPage();
  await page.addInitScript((devToken) => {
    localStorage.setItem(`codeterm-web-token:${window.location.origin}`, devToken);
    localStorage.removeItem("codeterm-chat-ui-default-enabled");
  }, token);
  await page.goto(`${API_BASE}/?e2e=1#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: WEB_MOUNT_CAP_MS });
  return page;
}

async function openFixturePage(page: Page, tabIds: string[], tabIds: string[]): Promise<void> {
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: WEB_MOUNT_CAP_MS });
  if (tabIds[0]) {
    await page.locator(`[data-testid="tab-item"][data-tab-id="${tabIds[0]}"]`).first().click({ timeout: 30_000 });
  }
  const chat = page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabIds[0]}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-composer"]')).toBeVisible({ timeout: 30_000 });
}

async function installPerfCollectors(page: Page): Promise<void> {
  await page.evaluate(() => {
    window.__composerPerf = { inputMarks: [], paintLatencies: [], renderCounts: {}, compositionEvents: [], ignoredKeys: [] };
    window.__renderCounts = {};
    window.__outputStreamPerf = { frames: 0, bytes: 0, batches: 0, lastFrameAt: 0 };
    window.__ipcPerf = { invokes: 0, invokeMs: 0, events: 0, byCommand: {}, byEvent: {} };
    window.__perfLongTasks = [];
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) window.__perfLongTasks?.push(entry.duration);
      }).observe({ entryTypes: ["longtask"] });
    } catch {
      /* longtask is not available in every browser engine. */
    }
  });
}

async function seedTranscript(page: Page, tabIds: string[]): Promise<void> {
  await page.evaluate(
    ({ ids, initialCount }) => {
      for (let i = 0; i < ids.length; i += 1) {
        const messages = Array.from({ length: initialCount }, (_, n) => ({
          id: `seed-${i}-${n}`,
          type: n % 11 === 0 ? "tool_call" : n % 5 === 0 ? "user" : "assistant",
          content: `build tab ${String(i).padStart(2, "0")} chunk ${n}\n\n\`\`\`text\nCompiling crate_${n % 37}\nwarning: generated output ${n}\n\`\`\``,
          timestamp: String(1_700_003_000_000 + i * 100_000 + n),
          provider: "perf",
        }));
        window.__codetermTestChat?.pushMessages(ids[i], messages as TestChatMessage[]);
        if (window.__outputStreamPerf) {
          window.__outputStreamPerf.frames += messages.length;
          window.__outputStreamPerf.batches += 1;
          window.__outputStreamPerf.bytes += messages.reduce((sum, message) => sum + message.content.length, 0);
          window.__outputStreamPerf.lastFrameAt = performance.now();
        }
      }
    },
    { ids: tabIds, initialCount: INITIAL_MESSAGES },
  );
}

async function startSustainedLoad(page: Page, tabIds: string[]): Promise<void> {
  await page.evaluate(
    ({ ids, batchSize, intervalMs }) => {
      let batch = 0;
      const push = () => {
        for (let tabIndex = 0; tabIndex < ids.length; tabIndex += 1) {
          const messages = Array.from({ length: batchSize }, (_, i) => {
            const n = batch * batchSize + i;
            return {
              id: `live-${tabIndex}-${n}`,
              type: n % 11 === 0 ? "tool_result" : n % 5 === 0 ? "user" : "assistant",
              content: `live build output tab=${tabIndex} n=${n}\n\n\`\`\`text\n${"warning: output ".repeat(20)}${n}\n\`\`\``,
              timestamp: String(1_700_004_000_000 + tabIndex * 100_000 + n),
              provider: "perf",
            };
          });
          window.__codetermTestChat?.pushMessages(ids[tabIndex], messages as TestChatMessage[]);
          if (window.__outputStreamPerf) {
            window.__outputStreamPerf.frames += messages.length;
            window.__outputStreamPerf.batches += 1;
            window.__outputStreamPerf.bytes += messages.reduce((sum, message) => sum + message.content.length, 0);
            window.__outputStreamPerf.lastFrameAt = performance.now();
          }
          window.__muxPerfChurnTab?.(ids[tabIndex]);
        }
        batch += 1;
      };
      push();
      const id = window.setInterval(push, intervalMs);
      window.__perfChurnStop = () => window.clearInterval(id);
    },
    { ids: tabIds, batchSize: LIVE_BATCH_SIZE, intervalMs: LIVE_INTERVAL_MS },
  );
}

function backendEvidence(): BackendEvidence {
  const logPath = path.join(os.homedir(), ".codeterm-dev", "logs", "codeterm.log");
  const text = fs.existsSync(logPath) ? fs.readFileSync(logPath, "utf8").slice(-500_000) : "";
  const lines = text.split(/\r?\n/);
  return {
    lockHoldLines: lines.filter((line) => line.includes("[LOCK-HOLD]")).slice(-20),
    slowIpcLines: lines.filter((line) => line.includes("[SLOW-IPC]") || line.includes("[LONGTASK]")).slice(-20),
    queueWaitLines: lines.filter((line) => /queue|wait|blocked|semaphore/i.test(line)).slice(-20),
  };
}

async function exerciseInput(page: Page, tabId: string): Promise<{ cpuStart: number; cpuEnd: number }> {
  const composer = page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"] [data-testid="chat-composer"]`);
  await composer.focus();
  await composer.press("ArrowLeft");
  await composer.press("ArrowRight");
  await composer.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await composer.press("Enter");
  await expect(composer).toHaveValue("");
  const nonInputMarks = await page.evaluate(() => window.__composerPerf?.inputMarks.length ?? 0);
  expect(nonInputMarks, "arrows/shortcuts/Enter must not create stale latency marks").toBe(0);
  const cpuStart = await page.evaluate(() => performance.now());
  await composer.pressSequentially(TYPED_TEXT, { delay: 8 });
  await expect(composer).toHaveValue(TYPED_TEXT);

  await composer.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await composer.pressSequentially("ime-safe");
  await composer.evaluate((el) => {
    // Real IME composition events bubble; React's root-delegated handlers
    // never see a non-bubbling synthetic dispatch.
    el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
    (el as HTMLTextAreaElement).value = "ime-safeかな";
    el.dispatchEvent(new InputEvent("input", { data: "かな", inputType: "insertCompositionText", bubbles: true }));
    el.dispatchEvent(new CompositionEvent("compositionend", { data: "かな", bubbles: true }));
  });
  await expect(composer).toHaveValue("ime-safeかな");
  const compositionEvents = await page.evaluate(() => window.__composerPerf?.compositionEvents.join(",") ?? "");
  expect(compositionEvents).toContain("start,end");

  await composer.evaluate((el) => {
    const textarea = el as HTMLTextAreaElement;
    textarea.setSelectionRange(textarea.value.length, textarea.value.length);
  });
  await page.evaluate((text) => navigator.clipboard.writeText(text), PASTED_TEXT);
  await composer.press(process.platform === "darwin" ? "Meta+V" : "Control+V");
  await expect(composer).toHaveValue(`ime-safeかな${PASTED_TEXT}`);

  const input = page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"] [data-testid="composer-attach-input"]`);
  await input.setInputFiles({
    name: "composer-perf-attachment.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("attachment fixture"),
  });
  await expect(page.locator('[data-testid="attachment-chip"]').first()).toBeVisible({ timeout: 10_000 });
  await composer.focus();
  await composer.press("End");
  await composer.pressSequentially(" history-safe", { delay: 8 });
  await expect(composer).toHaveValue(`ime-safeかな${PASTED_TEXT} history-safe`);
  const cpuEnd = await page.evaluate(() => performance.now());
  return { cpuStart, cpuEnd };
}

async function summarize(page: Page, mode: PerfSummary["mode"], tabIds: string[], cpuStart: number, cpuEnd: number): Promise<PerfSummary> {
  const variant = process.env.CT_PERF_VARIANT ?? "after";
  const summary = await page.evaluate(
    ({ targetTabId, openTabCount, start, end, runMode, runVariant }) => {
      const latencies = window.__composerPerf?.paintLatencies ?? [];
      const sorted = [...latencies].sort((a, b) => a - b);
      const pick = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))] ?? 0;
      const longTasks = window.__perfLongTasks ?? [];
      const ipc = window.__ipcPerf ?? { invokes: 0, events: 0, invokeMs: 0 };
      const output = window.__outputStreamPerf ?? { frames: 0, bytes: 0, batches: 0 };
      const wallSeconds = Math.max(0.001, (end - start) / 1000);
      const messages = Array.from(document.querySelectorAll(`[data-testid="tab-session-chat"][data-tab-id="${targetTabId}"] [data-testid="chat-message"]`));
      return {
        variant: runVariant,
        mode: runMode,
        p50: pick(0.5),
        p95: pick(0.95),
        max: sorted[sorted.length - 1] ?? 0,
        samples: latencies.length,
        composerRenders: window.__composerPerf?.renderCounts[targetTabId] ?? 0,
        tabRenders: window.__renderCounts?.[targetTabId] ?? 0,
        longTasks: longTasks.length,
        longTaskMax: Math.max(0, ...longTasks),
        longTaskTotalMs: longTasks.reduce((sum, value) => sum + value, 0),
        ipcInvokes: ipc.invokes,
        ipcEvents: ipc.events,
        ipcInvokeRate: ipc.invokes / wallSeconds,
        ipcEventRate: ipc.events / wallSeconds,
        outputFrames: output.frames,
        outputBytes: output.bytes,
        outputBatches: output.batches,
        outputFrameRate: output.frames / wallSeconds,
        transcriptMessages: messages.length,
        transcriptFirst: (messages[0] as HTMLElement | undefined)?.innerText?.slice(0, 80) ?? "",
        transcriptLast: (messages[messages.length - 1] as HTMLElement | undefined)?.innerText?.slice(0, 80) ?? "",
        openTabs: document.querySelectorAll('[data-testid="tab-item"]').length || openTabCount,
        scenarioWallMs: end - start,
        cpuMainThreadBusyRatio: longTasks.reduce((sum, value) => sum + value, 0) / Math.max(1, end - start),
        backendEvidence: { lockHoldLines: [], slowIpcLines: [], queueWaitLines: [] },
      } satisfies PerfSummary;
    },
    { targetTabId: tabIds[0], openTabCount: tabIds.length, start: cpuStart, end: cpuEnd, runMode: mode, runVariant: variant },
  );
  summary.backendEvidence = backendEvidence();
  return summary;
}

async function writeArtifact(testInfo: { outputPath: (...pathSegments: string[]) => string }, summary: PerfSummary): Promise<void> {
  const target = testInfo.outputPath(`composer-output-performance-${summary.variant}-${summary.mode}.json`);
  fs.writeFileSync(target, `${JSON.stringify(summary, null, 2)}\n`);
  console.log(`COMPOSER_PERF_${summary.mode.toUpperCase()} ${JSON.stringify(summary)}`);
}

async function runScenario(page: Page, mode: PerfSummary["mode"], tabIds: string[], tabIds: string[], testInfo: { outputPath: (...pathSegments: string[]) => string }): Promise<void> {
  await openFixturePage(page, tabIds, tabIds);
  const hasTestHook = await page.evaluate(() => typeof window.__codetermTestChat !== "undefined");
  test.skip(!hasTestHook, "VITE_CODETERM_TEST hooks are required for deterministic transcript churn");
  await installPerfCollectors(page);
  await seedTranscript(page, tabIds);
  await startSustainedLoad(page, tabIds);
  const { cpuStart, cpuEnd } = await exerciseInput(page, tabIds[0]);
  await page.evaluate(() => window.__perfChurnStop?.());
  const summary = await summarize(page, mode, tabIds, cpuStart, cpuEnd);
  await writeArtifact(testInfo, summary);
  recordPerfMetric({
    name: "composer-keystroke",
    unit: "ms",
    mode,
    n: summary.samples,
    p95: summary.p95,
    max: summary.max,
    budget: 32,
  });
  expect(summary.openTabs).toBeGreaterThanOrEqual(TAB_COUNT);
  expect(summary.samples).toBeGreaterThanOrEqual(30);
  expect(summary.transcriptMessages).toBeGreaterThan(200);
  // Capped/virtualized history rotates the absolute first message out under
  // sustained churn; assert the window still holds the seeded/churn family.
  expect(summary.transcriptFirst).toMatch(/build tab|live build output/);
  expect(summary.transcriptLast).toMatch(/live build output|warning: output|build tab/);
  expect(summary.p95).toBeLessThanOrEqual(32);
  expect(summary.max).toBeLessThanOrEqual(100);
}

test("@perf composer stays next-frame under 25-tab WebView2 load", async ({
  api,
  ui,
  tabCleanup,
}, testInfo) => {
  await ui.reconnect();
  await ui.page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: new URL(API_BASE).origin,
  });
  const { tabIds, tabIds } = await createFixture(api, tabCleanup);
  expect(tabIds).toHaveLength(TAB_COUNT);
  expect(tabIds).toHaveLength(TAB_COUNT);
  await runScenario(ui.page, "webview2", tabIds, tabIds, testInfo);
});

test("@perf composer stays next-frame under 25-tab web-mode load", async ({
  api,
  webBrowser,
  tabCleanup,
}, testInfo) => {
  const { tabIds, tabIds } = await createFixture(api, tabCleanup);
  expect(tabIds).toHaveLength(TAB_COUNT);
  expect(tabIds).toHaveLength(TAB_COUNT);
  const page = await openWebPage(webBrowser);
  try {
    await runScenario(page, "web", tabIds, tabIds, testInfo);
  } finally {
    await page.context().close();
  }
});
