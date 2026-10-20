// Switches sidebar tabs in Playwright Chromium and reports activation times and long tasks over 50 ms.
// `--profile FILE` also records a CPU profile of the switches and prints the heaviest functions.
// Never point it at prod.
//   node scripts/dev-perf/tabSwitch.mjs <frontend-url> --workspace NAME [--switches 10] [--profile out.cpuprofile] [--json]
import { writeFileSync } from "node:fs";
import { chromium } from "@playwright/test";
import { devApiToken } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback; };
const target = args.find((arg) => /^https?:/.test(arg));
const workspace = option("--workspace");
if (!target || !workspace) throw new Error("usage: tabSwitch.mjs <frontend-url> --workspace NAME [--switches N] [--profile FILE]");
const switchCount = Number(option("--switches", 10));
const profilePath = option("--profile");

const browser = await chromium.launch();
const page = await (await browser.newContext({ viewport: { width: 1280, height: 860 } })).newPage();
await page.goto(`${target.replace(/#.*$/, "")}#token=${encodeURIComponent(devApiToken())}`);
await page.waitForSelector(".ct-sidebar-content [data-dnd-group]", { timeout: 60_000 });
await page.waitForTimeout(3000);
const more = page.locator('[aria-label^="Show all workspaces"]');
if (await more.count()) await more.click();
await page.locator(`[aria-label="Select workspace ${workspace}"]`).first().click();
const fewer = page.locator('[aria-label="Show fewer workspaces"]');
if (await fewer.count()) await fewer.click();
await page.waitForTimeout(1500);
await page.evaluate(() => {
  window.__longTasks = [];
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) window.__longTasks.push(Math.round(entry.duration)); })
    .observe({ type: "longtask" });
});

const ids = await page.$$eval(".ct-sidebar-content [data-tab-id]", (rows) => rows.map((row) => row.dataset.tabId));
const cdp = profilePath ? await page.context().newCDPSession(page) : null;
if (cdp) {
  await cdp.send("Profiler.enable");
  await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
  await cdp.send("Profiler.start");
}
const activationMs = [];
const perSwitch = [];
for (let index = 0; index < switchCount; index++) {
  const id = ids[index % ids.length];
  const started = Date.now();
  await page.locator(`.ct-sidebar-content [data-tab-id="${id}"] button`).first().click();
  await page.waitForFunction((tabId) => document.querySelector(`.ct-sidebar-content [data-tab-id="${tabId}"] [aria-current=page]`), id, { timeout: 10_000 })
    .catch(() => {});
  activationMs.push(Date.now() - started);
  await page.waitForTimeout(700);
  perSwitch.push({ id, longTasks: await page.evaluate(() => window.__longTasks.splice(0)) });
}
const longTasks = perSwitch.flatMap((entry) => entry.longTasks);
const result = {
  url: target,
  switches: switchCount,
  activationMs,
  longTasksOver50: longTasks.filter((duration) => duration > 50).length,
  longest: Math.max(0, ...longTasks),
  perSwitch: perSwitch.map((entry) => `${entry.id}:${entry.longTasks.join("/") || "-"}`),
};
if (cdp) {
  const { profile } = await cdp.send("Profiler.stop");
  writeFileSync(profilePath, JSON.stringify(profile));
  result.heaviest = heaviestFunctions(profile, Number(option("--top", 25)));
}
await browser.close();
if (args.includes("--json")) console.log(JSON.stringify(result));
else {
  console.log(`activation=${activationMs.join(",")}ms longTasks>50=${result.longTasksOver50} longest=${result.longest}ms`);
  console.log(`per switch: ${result.perSwitch.join(" ")}`);
  if (result.heaviest) console.log("self ms / total ms  function:\n  " + result.heaviest.join("\n  "));
}

function heaviestFunctions(profile, top) {
  const nodes = new Map(profile.nodes.map((node) => [node.id, node]));
  const parents = new Map();
  for (const node of profile.nodes) for (const child of node.children ?? []) parents.set(child, node.id);
  const label = (node) => {
    const { functionName, url, lineNumber } = node.callFrame;
    return `${functionName || "(anonymous)"} ${url.replace(/^.*\/(src|node_modules)\//, "$1/").replace(/\?.*$/, "")}:${lineNumber + 1}`;
  };
  const self = new Map();
  const total = new Map();
  profile.samples.forEach((sampleId, index) => {
    const ms = (profile.timeDeltas[index + 1] ?? 0) / 1000;
    const leaf = nodes.get(sampleId);
    if (["(idle)", "(program)", "(garbage collector)"].includes(leaf.callFrame.functionName)) return;
    self.set(label(leaf), (self.get(label(leaf)) ?? 0) + ms);
    const seen = new Set();
    for (let id = sampleId; id !== undefined; id = parents.get(id)) {
      const name = label(nodes.get(id));
      if (seen.has(name)) continue;
      seen.add(name);
      total.set(name, (total.get(name) ?? 0) + ms);
    }
  });
  return [...total.entries()]
    .filter(([name]) => !name.startsWith("(root)"))
    .sort((a, b) => b[1] - a[1])
    .slice(0, top)
    .map(([name, ms]) => `${String(Math.round(self.get(name) ?? 0)).padStart(5)} / ${String(Math.round(ms)).padStart(5)}  ${name}`);
}
