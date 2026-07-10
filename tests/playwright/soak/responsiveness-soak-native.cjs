// Native WebView2 responsiveness soak over the real desktop app's CDP endpoint.
// Mirrors the web-mode soak metrics so native and web runs are comparable.
const { chromium } = require("@playwright/test");
const fs = require("node:fs");
const path = require("node:path");

const CDP_PORT = process.env.SOAK_CDP_PORT || "9223";
const CDP_BASE = `http://127.0.0.1:${CDP_PORT}`;
const OUT = process.env.SOAK_OUT || "/tmp/soak-artifacts-native";
const DURATION_MS = Number(process.env.SOAK_DURATION_MS || 300000);
const TARGET_WORKSPACE_ID = process.env.SOAK_WORKSPACE_ID || "";

async function connectDesktop() {
  const browser = await chromium.connectOverCDP(CDP_BASE);
  const pages = browser
    .contexts()
    .flatMap((c) => c.pages())
    .filter((p) => !p.isClosed());
  const page =
    pages.find((p) => /localhost|tauri/.test(p.url())) ||
    pages[0] ||
    (() => {
      throw new Error("no page exposed over CDP - dev WebView2 window missing?");
    })();
  return { browser, page };
}

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const { browser, page } = await connectDesktop();
  const context = page.context();
  try {
    await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  } catch (e) {
    console.error("tracing.start failed (non-fatal over CDP):", String(e));
  }
  const consoleErrors = [];
  const slowIpcConsole = [];
  page.on("console", (m) => {
    const t = m.text();
    if (m.type() === "error") consoleErrors.push(t.slice(0, 300));
    if (/\[SLOW-IPC\]|\[LONGTASK\]/.test(t)) slowIpcConsole.push(t.slice(0, 300));
  });

  await page.waitForSelector("#root > *", { state: "attached", timeout: 90000 });
  await page
    .locator('.xterm:visible, [data-testid="chat-composer"], [data-testid="tab-item"]')
    .first()
    .waitFor({ state: "visible", timeout: 90000 })
    .catch(() => {});

  // rAF-gap + longtask monitor (identical to the web-mode driver).
  await page.evaluate(() => {
    const w = window;
    w.__soak = { longtasks: [], maxRafGap: 0, rafGaps: [], frames: 0, start: performance.now(), wallStart: Date.now() };
    try {
      const po = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          w.__soak.longtasks.push({ start: Math.round(e.startTime), dur: Math.round(e.duration) });
        }
      });
      po.observe({ entryTypes: ["longtask"] });
    } catch (e) {
      w.__soak.longtaskErr = String(e);
    }
    let last = performance.now();
    const tick = (now) => {
      const gap = now - last;
      last = now;
      w.__soak.frames++;
      if (gap > w.__soak.maxRafGap) w.__soak.maxRafGap = gap;
      if (gap > 100) w.__soak.rafGaps.push({ t: Math.round(now), gap: Math.round(gap) });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  const t0 = Date.now();
  const interactionStartWall = await page.evaluate(() => Date.now());
  let shot = 0;
  const snap = async (label) => {
    try {
      await page.screenshot({ path: path.join(OUT, `native-${String(shot++).padStart(2, "0")}-${label}.png`) });
    } catch (e) {
      /* ignore */
    }
  };
  const busy = { tabActivity: false, activeExecutor: false, boardLoading: false, statusChip: false, tabStatusDot: false };
  const planEntryLocator = () =>
    TARGET_WORKSPACE_ID
      ? page.locator(`[data-testid="workspace-item"][data-workspace-id="${TARGET_WORKSPACE_ID}"] [data-testid="workspace-plan-entry"]`).first()
      : page.locator('[data-testid="workspace-plan-entry"]').first();
  const openPlanSurface = async () => {
    const planEntry = planEntryLocator();
    if (await planEntry.count().catch(() => 0)) {
      await planEntry.click({ timeout: 2000 }).catch(() => {});
      return;
    }
    if (!(await page.locator('[data-testid="plan-board-view"], [data-testid="plan-view-shell"]').count().catch(() => 0))) {
      await page.locator('[data-testid="canvas-toolbar-button"]').click({ timeout: 2000 }).catch(() => {});
    }
  };
  const noteBusy = async () => {
    if (await page.locator('[data-testid="tab-activity"]').count().catch(() => 0)) busy.tabActivity = true;
    if (await page.locator('[data-testid="plan-board-task-active-executor"]').count().catch(() => 0)) busy.activeExecutor = true;
    if (await page.locator('[data-testid="plan-board-loading"]').count().catch(() => 0)) busy.boardLoading = true;
    if (await page.locator('[data-testid="status-chip"]').count().catch(() => 0)) busy.statusChip = true;
    if (await page.locator('[data-testid="tab-status-rail"]').count().catch(() => 0)) busy.tabStatusDot = true;
  };

  await openPlanSurface();
  await page
    .locator('[data-testid="plan-board-view"], [data-testid="plan-view-shell"], [data-testid="canvas-world-canvas"]')
    .first()
    .waitFor({ state: "visible", timeout: 10000 })
    .catch(() => {});
  await noteBusy();
  await snap("start");
  let iter = 0;
  while (Date.now() - t0 < DURATION_MS) {
    iter++;
    const tabs = page.locator('[data-testid="tab-item"]');
    const n = await tabs.count().catch(() => 0);
    if (n > 0) await tabs.nth(iter % n).click({ timeout: 2000 }).catch(() => {});
    await page.waitForTimeout(250);

    await openPlanSurface();
    await page.waitForTimeout(250);

    const scrollable = page.locator('[data-testid="plan-board-view"], [data-testid="plan-skills-section"]').first();
    if (await scrollable.count().catch(() => 0)) {
      await scrollable.evaluate((el) => el.scrollBy(0, 500)).catch(() => {});
      await page.waitForTimeout(120);
      await scrollable.evaluate((el) => el.scrollBy(0, -260)).catch(() => {});
    } else {
      await page.mouse.wheel(0, 500).catch(() => {});
    }
    await page.waitForTimeout(150);
    await noteBusy();
    if (iter % 8 === 0) await snap(`iter${iter}`);
  }
  await noteBusy();
  await snap("end");
  const interactionEndWall = await page.evaluate(() => Date.now());

  const stats = await page.evaluate(() => {
    const s = window.__soak;
    const gaps = s.rafGaps;
    const perf = window.__ctPerf || [];
    return {
      durationMs: Math.round(performance.now() - s.start),
      frames: s.frames,
      maxRafGapMs: Math.round(s.maxRafGap),
      rafGapsOver100: gaps,
      rafGapsOver250: gaps.filter((g) => g.gap > 250),
      longtaskCount: s.longtasks.length,
      longtaskMaxMs: s.longtasks.reduce((m, l) => Math.max(m, l.dur), 0),
      longtasksOver250: s.longtasks.filter((l) => l.dur > 250),
      longtaskErr: s.longtaskErr || null,
      // window.__ctPerf entries from src/lib/bridge.ts: {cmd, ms, at}
      ctPerfAll: perf,
      slowIpc: perf.filter((p) => p.cmd !== "LONGTASK"),
      slowIpcLongtask: perf.filter((p) => p.cmd === "LONGTASK"),
    };
  });

  const firstSlowResolver =
    stats.slowIpc.filter((p) => p.at >= interactionStartWall && p.at <= interactionEndWall).sort((a, b) => a.at - b.at)[0] || null;

  const result = {
    mode: "native-webview2-cdp",
    cdpBase: CDP_BASE,
    targetWorkspaceId: TARGET_WORKSPACE_ID || null,
    interactionStartWall,
    interactionEndWall,
    ...stats,
    firstSlowResolver,
    slowIpcConsole,
    busyStatesSeen: busy,
    iterations: iter,
    consoleErrors,
  };
  fs.writeFileSync(path.join(OUT, "soak-native-stats.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ ...result, ctPerfAll: `[${stats.ctPerfAll.length} entries]`, rafGapsOver100: `[${stats.rafGapsOver100.length}]` }, null, 2));
  try {
    await context.tracing.stop({ path: path.join(OUT, "soak-native-trace.zip") });
  } catch (e) {
    console.error("tracing.stop failed:", String(e));
  }
  // Do NOT browser.close() - that would close the real dev app. Just disconnect.
  await browser.close().catch(() => {});
}

main().catch((e) => {
  console.error("SOAK-NATIVE-FAILED", e);
  process.exit(1);
});
