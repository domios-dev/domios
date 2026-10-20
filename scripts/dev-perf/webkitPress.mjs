// Presses every visible sidebar row in Playwright WebKit across a sweep of scroll positions and reports
// scroll jumps, wrong-target activations and row remounts. Cocoa WebKit never mouse-focuses a button, so
// the probe focuses the nearest focusable ancestor on an unprevented press, as the macOS app does.
// Never point it at prod. Needs `node node_modules/@playwright/test/cli.js install webkit` once.
//   node scripts/dev-perf/webkitPress.mjs <frontend-url> [--steps 10] [--workspace NAME] [--json]
import { webkit } from "@playwright/test";
import { devApiToken } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback; };
const target = args.find((arg) => /^https?:/.test(arg));
if (!target) throw new Error("usage: webkitPress.mjs <frontend-url> [--steps N] [--workspace NAME]");
const steps = Number(option("--steps", 10));
const workspace = option("--workspace");

const browser = await webkit.launch();
const page = await (await browser.newContext({ viewport: { width: 1280, height: 860 } })).newPage();
await page.addInitScript(() => {
  const mouseFocusable = (el) => el instanceof HTMLElement && el.hasAttribute("tabindex") && el.tabIndex >= 0
    && !el.matches("button, select, input[type=button], input[type=submit]");
  window.addEventListener("mousedown", (event) => {
    window.__press = { prevented: event.defaultPrevented, focusedWrapper: false };
    if (event.defaultPrevented) return;
    let el = event.target;
    while (el && !mouseFocusable(el)) el = el.parentElement;
    if (el) { window.__press.focusedWrapper = true; el.focus(); }
  });
});
await page.goto(`${target.replace(/#.*$/, "")}#token=${encodeURIComponent(devApiToken())}`);
await page.waitForSelector(".ct-sidebar-content [data-dnd-group]", { timeout: 60000 });
if (workspace) {
  const more = page.locator('[aria-label^="Show all workspaces"]');
  if (await more.count()) await more.click();
  await page.locator(`[aria-label="Select workspace ${workspace}"]`).first().click();
  const fewer = page.locator('[aria-label="Show fewer workspaces"]');
  if (await fewer.count()) await fewer.click();
}
for (const toggle of await page.locator(".ct-sidebar-content button", { hasText: /utility shells/i }).all()) {
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
}
// The list must stop growing before a press is judged, or its own growth reads as a jump.
let scrollMax = -1;
for (let attempt = 0; attempt < 40; attempt++) {
  const next = await page.$eval(".ct-sidebar-content", (el) => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(el.scrollHeight - el.clientHeight)))));
  if (next === scrollMax) break;
  scrollMax = next;
}

const results = [];
for (let step = 0; step <= steps; step++) {
  const scrollTarget = Math.round((scrollMax * step) / steps);
  const rows = await page.evaluate((top) => {
    const scroller = document.querySelector(".ct-sidebar-content");
    scroller.scrollTop = top;
    const box = scroller.getBoundingClientRect();
    return [...scroller.querySelectorAll("[data-tab-id]")].map((el) => {
      const rect = el.getBoundingClientRect();
      return { id: el.dataset.tabId, x: rect.left + Math.min(70, rect.width / 3), y: rect.top + Math.min(12, rect.height / 2) };
    }).filter((row) => row.y > box.top + 75 && row.y < box.bottom - 4
      && document.elementFromPoint(row.x, row.y)?.closest("[data-tab-id]")?.dataset.tabId === row.id);
  }, scrollTarget);
  for (const row of rows) {
    const before = await page.evaluate(({ top, id }) => {
      const scroller = document.querySelector(".ct-sidebar-content");
      document.activeElement?.blur?.();
      scroller.scrollTop = top;
      scroller.querySelector(`[data-tab-id="${id}"]`).__pressMark = true;
      return scroller.scrollTop;
    }, { top: scrollTarget, id: row.id });
    await page.mouse.move(row.x, row.y);
    await page.mouse.down();
    const during = await page.$eval(".ct-sidebar-content", (el) => el.scrollTop);
    await page.mouse.up();
    const after = await page.evaluate((id) => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => {
      const scroller = document.querySelector(".ct-sidebar-content");
      const active = scroller.querySelector("[data-tab-id] [aria-current=page]")?.closest("[data-tab-id]")?.dataset.tabId ?? null;
      resolve({ scrollTop: scroller.scrollTop, remounted: !scroller.querySelector(`[data-tab-id="${id}"]`)?.__pressMark, active, press: window.__press });
    }))), row.id);
    results.push({
      scroll: before,
      row: row.id,
      jump: Math.max(Math.abs(during - before), Math.abs(after.scrollTop - before)),
      activated: after.active === row.id,
      remounted: after.remounted,
      focusedWrapper: Boolean(after.press?.focusedWrapper),
    });
  }
}
await browser.close();

const jumps = results.filter((entry) => entry.jump > 0);
const summary = {
  url: target,
  scrollMax,
  presses: results.length,
  scrollJumps: jumps.length,
  maxJump: Math.max(0, ...jumps.map((entry) => entry.jump)),
  wrongTarget: results.filter((entry) => !entry.activated).length,
  remounts: results.filter((entry) => entry.remounted).length,
  wrapperFocused: results.filter((entry) => entry.focusedWrapper).length,
};
console.log(JSON.stringify(args.includes("--json") ? { summary, results } : summary, null, 1));
