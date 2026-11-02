import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { chromium, webkit } from "@playwright/test";

const require = createRequire(import.meta.url);
const { PNG } = require(require.resolve("pngjs", { paths: [path.dirname(require.resolve("@playwright/test"))] }));

const baseUrl = process.env.SIDEBAR_SCROLL_URL ?? "http://127.0.0.1:1427/tests/fixtures/sidebar-scroll/index.html";
const evidenceDir = process.env.SIDEBAR_SCROLL_SCREENSHOT_DIR ?? path.join(tmpdir(), "codeterm-sidebar-scroll-webkit");
await mkdir(evidenceDir, { recursive: true });

for (const [engine, browserType] of Object.entries({ webkit, chromium })) {
  const screenshotDir = path.join(evidenceDir, engine);
  await mkdir(screenshotDir, { recursive: true });

  const browser = await browserType.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 900, height: 680 }, deviceScaleFactor: 1 });
  page.setDefaultTimeout(15_000);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route("**/api/**", route => {
    pageErrors.push(`Unexpected backend request: ${new URL(route.request().url()).pathname}`);
    return route.abort();
  });

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

  async function measure() {
    return page.evaluate(() => {
      const viewport = document.querySelector("[data-testid=viewport]");
      return {
        scrollTop: viewport.scrollTop,
        maximum: viewport.scrollHeight - viewport.clientHeight,
        boxes: Object.fromEntries([...document.querySelectorAll("[data-tab-id], [data-sticky]")].map(element => {
          const rect = element.getBoundingClientRect();
          return [element.dataset.tabId ?? element.dataset.sticky, [rect.x, rect.y, rect.width, rect.height]];
        })),
      };
    });
  }

  const results = [];
  const paint = [];
  try {
    await page.goto(baseUrl);
    await page.getByTestId("viewport").waitFor();
    await frames();
    await page.screenshot({ path: path.join(screenshotDir, "initial.png") });
    const external = (workspace) => page.evaluate(value => window.sidebarFixture.set({ workspace: value, tab: 0, generalTab: null }), workspace);
    const scrollTo = async (top) => {
      await page.evaluate(value => { document.querySelector("[data-testid=viewport]").scrollTop = value; }, top);
      await frames();
    };
    for (const workspace of [2, 3, 4, 1, 0, 2, 0, 4, 3, 1]) {
      const before = await measure();
      await external(workspace);
      await frames();
      const after = await measure();
      const label = `workspace-${results.length}-${workspace}`;
      const geometry = await headerGeometry(label);
      paint.push({ label, geometry, mismatch: mismatch(geometry) });
      results.push({ label, before, after });
      assert.equal(after.scrollTop, Math.min(before.scrollTop, after.maximum), "workspace replacement only clamps an exhausted native scroll range");
      if (before.scrollTop === after.scrollTop) {
        for (const id of ["general", "general-0", "general-1", "workspaces", "detail"]) {
          assert.deepEqual(after.boxes[id], before.boxes[id], `${label}: persistent row/header ${id} stays still`);
        }
      }
      for (const offset of [0, 30, 100, 250]) {
        await scrollTo(offset);
        const targets = await page.locator("[aria-label^='Open tab ']").evaluateAll(elements => elements.filter(element => {
          const rect = element.getBoundingClientRect();
          const bounds = element.closest("[data-testid=viewport]").getBoundingClientRect();
          return rect.top >= bounds.top && rect.bottom <= bounds.bottom
            && element.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2));
        }).slice(0, 4).map(element => element.getAttribute("aria-label")));
        assert(targets.length > 0, "every scroll offset has an unobscured tab target");
        for (const name of targets) {
          const before = await measure();
          const target = page.getByRole("button", { name, exact: true });
          const box = await target.boundingBox();
          await target.evaluate(element => { window.selectedRowIdentity = element.closest("[data-tab-id]"); });
          // Real pointer input avoids locator auto-scrolling, which can itself move a sticky-obscured target.
          await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
          await page.waitForFunction(() => window.selectedRowIdentity.querySelector("[aria-current=page]"));
          await frames();
          const after = await measure();
          results.push({ name, workspace, offset, before, after });
          assert.equal(await target.evaluate(element => element.closest("[data-tab-id]") === window.selectedRowIdentity), true, "selection retains the existing row node");
          assert.equal(after.scrollTop, before.scrollTop, `${engine}: selecting ${name} preserves scrollTop`);
          assert.deepEqual(after.boxes, before.boxes, `${engine}: selecting ${name} moves no rows or headers`);
        }
      }
    }
    assert.deepEqual(pageErrors, [], "fixture renders without browser errors");
    assert.deepEqual(paint.filter(result => result.mismatch), [], "sticky header paint matches its layout after workspace replacements");
    const selections = results.filter(result => result.name).length;
    assert(selections >= 40, "the focused invariant exercises multiple workspaces and scroll offsets");
    console.log(`PASS ${engine}: ${selections} real-row selections, zero box/scroll movement; ${paint.length} workspace replacements, native range clamps only, header paint matches layout; ${screenshotDir}`);
  } finally {
    await writeFile(path.join(screenshotDir, "measurements.json"), JSON.stringify({ results, paint, pageErrors }, null, 2));
    await browser.close();
  }
}
