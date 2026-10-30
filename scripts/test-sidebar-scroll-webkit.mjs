import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { webkit } from "@playwright/test";

const require = createRequire(import.meta.url);
const { PNG } = require(require.resolve("pngjs", { paths: [path.dirname(require.resolve("@playwright/test"))] }));

const baseUrl = process.env.SIDEBAR_SCROLL_URL ?? "http://127.0.0.1:1427/tests/fixtures/sidebar-scroll/index.html";
const screenshotDir = process.env.SIDEBAR_SCROLL_SCREENSHOT_DIR ?? path.join(tmpdir(), "codeterm-sidebar-scroll-webkit");
await mkdir(screenshotDir, { recursive: true });

const browser = await webkit.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 680 }, deviceScaleFactor: 1 });
page.setDefaultTimeout(15_000);
const pageErrors = [];
page.on("pageerror", (error) => pageErrors.push(error.message));

const frames = () => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));

/** Sticky header tops as laid out (DOM) and as painted (magenta runs in the screenshot), both viewport-relative. */
async function headerGeometry(label) {
  const layout = await page.evaluate(() => {
    const viewport = document.querySelector("[data-testid=viewport]");
    const bounds = viewport.getBoundingClientRect();
    return {
      viewportTop: bounds.top,
      viewportBottom: bounds.bottom,
      centerX: bounds.left + bounds.width / 2,
      scrollTop: viewport.scrollTop,
      maximum: viewport.scrollHeight - viewport.clientHeight,
      trailingSpace: document.querySelector("[data-testid=trailing-space]").offsetHeight,
      headers: [...document.querySelectorAll("[data-sticky]")].map((element) => {
        const rect = element.getBoundingClientRect();
        return { id: element.dataset.sticky, top: Math.round(rect.top - bounds.top), bottom: Math.round(rect.bottom - bounds.top) };
      }).filter((header) => header.bottom > 0 && header.top < bounds.height),
    };
  });
  const buffer = await page.screenshot({ path: path.join(screenshotDir, `${label}.png`) });
  const png = PNG.sync.read(buffer);
  const x = Math.round(layout.centerX);
  const runs = [];
  let start = null;
  for (let y = Math.round(layout.viewportTop); y < Math.round(layout.viewportBottom); y += 1) {
    const offset = (png.width * y + x) * 4;
    const magenta = png.data[offset] > 200 && png.data[offset + 1] < 60 && png.data[offset + 2] > 200;
    if (magenta && start === null) start = y;
    if (!magenta && start !== null) { runs.push({ top: start - Math.round(layout.viewportTop), bottom: y - Math.round(layout.viewportTop) }); start = null; }
  }
  if (start !== null) runs.push({ top: start - Math.round(layout.viewportTop), bottom: Math.round(layout.viewportBottom - layout.viewportTop) });
  return { ...layout, painted: runs };
}

function mismatch(geometry) {
  const expected = [];
  for (const header of [...geometry.headers].sort((left, right) => left.top - right.top)) {
    const top = Math.max(0, header.top);
    const last = expected.at(-1);
    if (last && top <= last[1] + 1) last[1] = Math.max(last[1], header.bottom);
    else expected.push([top, header.bottom]);
  }
  const painted = geometry.painted.map((run) => [run.top, run.bottom]);
  if (expected.length !== painted.length) return true;
  return expected.some(([top, bottom], index) => Math.abs(top - painted[index][0]) > 1 || Math.abs(bottom - painted[index][1]) > 1);
}

try {
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.getByTestId("viewport").waitFor();
  await page.evaluate(() => {
    const log = [];
    const viewport = document.querySelector("[data-testid=viewport]");
    Object.assign(window, { sidebarScrollLog: log });
    document.addEventListener("sidebar-scroll-debug", (event) => {
      const entry = { ...event.detail };
      log.push(entry);
      // The write event precedes the assignment; the observer callback finishes before this microtask.
      if (entry.edge === "write") queueMicrotask(() => { entry.landed = viewport.scrollTop; });
    });
  });
  const results = [];
  const record = async (label) => {
    await frames();
    const geometry = await headerGeometry(label);
    const events = await page.evaluate(() => window.sidebarScrollLog.splice(0));
    results.push({ label, geometry, events, mismatch: mismatch(geometry) });
  };
  const external = (workspace, tab) => page.evaluate(([w, t]) => window.sidebarFixture.set({ workspace: w, tab: t }), [workspace, tab]);
  const click = async (workspaceTab) => {
    await page.getByRole("button", { name: `Open tab ${workspaceTab}`, exact: true }).click();
  };
  const scrollTo = (top) => page.evaluate((value) => new Promise((resolve) => {
    const viewport = document.querySelector("[data-testid=viewport]");
    const before = viewport.scrollTop;
    viewport.scrollTop = value;
    if (viewport.scrollTop === before) return resolve();
    viewport.addEventListener("scroll", () => resolve(), { once: true });
  }), top);

  await record("initial");
  const workspaces = await page.evaluate(() => window.sidebarFixture.workspaces);
  // Long and short workspaces alternate and each switch starts from a different scroll offset, mid-list included.
  const order = [2, 3, 4, 1, 0];
  const offsets = [0, 100, 10_000, 60, 200, 30];
  for (let index = 0; index < 30; index += 1) {
    const workspace = order[index % order.length];
    const target = workspaces[workspace];
    await scrollTo(offsets[index % offsets.length]);
    await record(`switch-${index}-from`);
    if (index % 2) await external(workspace, index % target.tabs);
    else {
      await external(workspace, 0);
      await record(`switch-${index}-workspace`);
      const tab = `${target.id}-${(index + 1) % target.tabs}`;
      if (await page.getByRole("button", { name: `Open tab ${tab}`, exact: true }).isVisible()) await click(tab);
    }
    await record(`switch-${index}`);
  }

  const writes = results.flatMap((result) => result.events.filter((event) => event.edge === "write").map((event) => ({ label: result.label, ...event })));
  const unreachable = writes.filter((write) => Math.abs(write.landed - write.desiredScrollTop) > 0.5);
  for (const result of results.filter((entry) => entry.mismatch)) {
    const g = result.geometry;
    console.log(`MISMATCH ${result.label} scrollTop=${g.scrollTop} space=${g.trailingSpace} layout=${JSON.stringify(g.headers.map((h) => [h.id, Math.max(0, h.top)]))} painted=${JSON.stringify(g.painted.map((run) => run.top))}`);
  }
  for (const write of unreachable) console.log(`UNREACHABLE ${write.label} desired=${write.desiredScrollTop} landed=${write.landed}`);
  const gaps = results.filter((result) => result.geometry.maximum === 0 && result.geometry.trailingSpace > 0).map((result) => result.label);
  assert.deepEqual(pageErrors, [], "fixture renders without browser errors");
  assert.deepEqual(unreachable, [], "every anchor write lands at the offset it asked for");
  assert.deepEqual(gaps, [], "content that fits the viewport keeps no trailing space");
  const mismatched = results.filter((result) => result.mismatch).map((result) => result.label);
  assert.deepEqual(mismatched, [], "WebKit paints every sticky header where layout puts it after each tab switch");
  console.log(`PASS WebKit: ${results.length} tab-switch states, ${writes.length} anchor writes all reachable, sticky headers painted at their layout positions; screenshots saved to ${screenshotDir}`);
} finally {
  await browser.close();
}
