/*
 * Focused plan-board responsiveness soak (web-mode, 7702).
 * Opens the workspace canvas / plan board and continuously switches the
 * Board/List/Canvas view, scrolls the board, and switches tabs while
 * assigned agents work. Captures a Playwright trace + longtask / rAF-gap
 * monitor and plan-board busy-state coverage. Measurement lane only.
 */
const { chromium } = require("@playwright/test");
const { devConnection } = require("../../../evals/_lib/connection.cjs");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const PORT = process.env.SOAK_PORT || "7702";
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = process.env.SOAK_OUT || "/tmp/soak-artifacts";
const DURATION_MS = Number(process.env.SOAK_DURATION_MS || 120000);
const DATADIR = process.env.SOAK_DATADIR || path.join(os.homedir(), ".domios-dev-soak");

function readToken() {
  return devConnection({ dataDir: DATADIR, apiPort: Number(PORT) }).token;
}

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const token = readToken();
  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  const page = await context.newPage();
  await page.addInitScript((t) => {
    try {
      localStorage.setItem(`codeterm-web-token:${location.origin}`, t);
      localStorage.removeItem("codeterm-chat-ui-default-enabled");
    } catch (e) {
      /* ignore */
    }
  }, token);
  await page.goto(`${BASE}/#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90000 });
  await page.locator('[data-testid="canvas-toolbar-button"]').first().click({ timeout: 20000 }).catch(() => {});
  await page.locator('[data-testid="plan-board-view"], [data-testid="plan-view-mode-switch"]').first().waitFor({ state: "visible", timeout: 30000 }).catch(() => {});

  await page.evaluate(() => {
    const w = window;
    w.__soak = { longtasks: [], maxRafGap: 0, rafGaps: [], frames: 0, start: performance.now() };
    try {
      const po = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) w.__soak.longtasks.push({ start: Math.round(e.startTime), dur: Math.round(e.duration) });
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
  let shot = 0;
  const snap = async (label) => {
    try {
      await page.screenshot({ path: path.join(OUT, `board-${String(shot++).padStart(2, "0")}-${label}.png`) });
    } catch (e) {
      /* ignore */
    }
  };
  const busy = { activeExecutor: false, boardLoading: false, statusChip: false, tabStatusDot: false };
  const noteBusy = async () => {
    if (await page.locator('[data-testid="plan-board-task-active-executor"]').count().catch(() => 0)) busy.activeExecutor = true;
    if (await page.locator('[data-testid="plan-board-loading"]').count().catch(() => 0)) busy.boardLoading = true;
    if (await page.locator('[data-testid="status-chip"]').count().catch(() => 0)) busy.statusChip = true;
    if (await page.locator('[data-testid="tab-status-rail"]').count().catch(() => 0)) busy.tabStatusDot = true;
  };

  await snap("start");
  let iter = 0;
  const modes = page.locator('[data-testid="plan-view-mode-switch"] [role="tab"]');
  const modeCount = await modes.count().catch(() => 0);
  while (Date.now() - t0 < DURATION_MS) {
    iter++;
    if (modeCount > 0) await modes.nth(iter % modeCount).click({ timeout: 2000 }).catch(() => {});
    await page.waitForTimeout(250);
    const col = page.locator('[data-testid="plan-board-column"], [data-testid="plan-board-view"]').first();
    if (await col.count().catch(() => 0)) {
      await col.evaluate((el) => el.scrollBy(0, 600)).catch(() => {});
      await page.waitForTimeout(120);
      await col.evaluate((el) => el.scrollBy(0, -300)).catch(() => {});
    } else {
      await page.mouse.wheel(0, 600).catch(() => {});
    }
    await page.waitForTimeout(150);
    const tabs = page.locator('[data-testid="tab-item"]');
    const n = await tabs.count().catch(() => 0);
    if (n > 0) await tabs.nth(iter % n).click({ timeout: 2000 }).catch(() => {});
    await page.waitForTimeout(200);
    await noteBusy();
    if (iter % 6 === 0) await snap(`iter${iter}`);
  }
  await noteBusy();
  await snap("end");

  const stats = await page.evaluate(() => {
    const s = window.__soak;
    return {
      durationMs: Math.round(performance.now() - s.start),
      frames: s.frames,
      maxRafGapMs: Math.round(s.maxRafGap),
      rafGapsOver250: s.rafGaps.filter((g) => g.gap > 250),
      longtaskCount: s.longtasks.length,
      longtaskMaxMs: s.longtasks.reduce((m, l) => Math.max(m, l.dur), 0),
      longtasksOver250: s.longtasks.filter((l) => l.dur > 250),
      longtaskErr: s.longtaskErr || null,
    };
  });
  const result = { ...stats, busyStatesSeen: busy, iterations: iter };
  fs.writeFileSync(path.join(OUT, "soak-board-stats.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
  await context.tracing.stop({ path: path.join(OUT, "soak-board-trace.zip") });
  await browser.close();
}

main().catch((e) => {
  console.error("SOAK-BOARD-FAILED", e);
  process.exit(1);
});
