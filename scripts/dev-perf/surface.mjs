// Drives one interface surface with real CDP input and reports React renders per component and long tasks.
// Run open.mjs first (or reload with the render hook) and keep agents streaming for load.
//   node scripts/dev-perf/surface.mjs <scenario> [--tab TAB_ID] [--port CDP_PORT] [--url-match ORIGIN] [--json]
// Scenarios: transcript-scroll, sidebar-scroll, sidebar-hover, stream-idle, mesh-click, mesh-refresh,
//   memory-scroll, memory-graph, plan-scroll, analytics-open, notifications-open, settings-tabs
import { connectPage, sleep } from "./cdp.mjs";
import { READ_RENDERS, RESET_RENDERS } from "./renderHook.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const scenario = args[0];
const page = await connectPage(option("--port") ? Number(option("--port")) : undefined, option("--url-match"));
if (!(await page.evaluate("!!window.__devPerfRenders"))) throw new Error("render hook missing; run open.mjs first");

const mouse = (type, x, y, extra = {}) => page.call("Input.dispatchMouseEvent", { type, x, y, button: "none", ...extra });
const click = async (x, y) => {
  await mouse("mouseMoved", x, y);
  await mouse("mousePressed", x, y, { button: "left", buttons: 1, clickCount: 1 });
  await mouse("mouseReleased", x, y, { button: "left", buttons: 0, clickCount: 1 });
};
// Hidden panels stay mounted; only an element painted on screen counts.
const SHOWN = `(candidate) => { const r = candidate.getBoundingClientRect(); return r.width > 0 && r.height > 0 && candidate.checkVisibility({ visibilityProperty: true, opacityProperty: true }); }`;
const rect = (selector) => page.evaluate(`(() => {
  const element = [...document.querySelectorAll(${JSON.stringify(selector)})].find(${SHOWN});
  if (!element) return null;
  const r = element.getBoundingClientRect();
  return { x: r.left + r.width / 2, y: r.top + r.height / 2, left: r.left, top: r.top, width: r.width, height: r.height };
})()`);
const need = async (selector) => {
  const found = await rect(selector);
  if (!found) throw new Error(`${scenario}: nothing visible matches ${selector}`);
  return found;
};
const clickSelector = async (selector) => {
  await page.evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].find(${SHOWN})?.scrollIntoView({ block: "nearest" })`);
  await frame();
  const r = await need(selector);
  await click(r.x, r.y);
  return r;
};
const frame = () => page.evaluate("new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
const settle = async (ms = 600) => { await sleep(ms); await frame(); };
const wheel = async (target, deltaY, strokes) => {
  // A phone scrolls by touch: one finger swipe covering the same distance.
  if (option("--width")) {
    const swipes = Math.ceil(Math.abs(deltaY * strokes) / 300);
    const step = deltaY > 0 ? -15 : 15;
    for (let swipe = 0; swipe < swipes; swipe++) {
      const startY = target.y - step * 10;
      await page.call("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: target.x, y: startY }] });
      for (let move = 1; move <= 20; move++) {
        await page.call("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: target.x, y: startY + step * move }] });
        await sleep(16);
      }
      await page.call("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    }
    return;
  }
  for (let stroke = 0; stroke < strokes; stroke++) {
    await mouse("mouseWheel", target.x, target.y, { deltaX: 0, deltaY });
    await sleep(16);
  }
};
const selectWorkspace = async (name) => {
  const tile = `[aria-label="Select workspace ${name}"]`;
  if (!(await rect(tile))) await clickSelector(`[aria-label^="Show all workspaces"]`);
  await settle(300);
  await clickSelector(tile);
  await settle(800);
};
const activateTab = async (tabId) => {
  if (!(await rect(`[data-tab-id="${tabId}"]`)) && option("--workspace")) await selectWorkspace(option("--workspace"));
  await clickSelector(`[data-tab-id="${tabId}"]`);
  await page.evaluate(`new Promise((resolve, reject) => { const started = performance.now(); const check = () => {
    if (document.querySelector('[data-testid=tab-session-chat-messages]')?.offsetParent) resolve(true);
    else if (performance.now() - started > 15000) reject(new Error("chat did not mount"));
    else requestAnimationFrame(check); }; check(); })`);
  await settle(1500);
};
const LARGEST_LOG = `[...document.querySelectorAll("[data-testid=tab-session-chat-messages]")].filter((log) => log.offsetParent).sort((a, b) => b.scrollHeight - a.scrollHeight)[0]`;
const VISIBLE_LOG = "[data-testid=tab-session-chat-messages]";
const TRANSCRIPT_STATE = `(() => { const log = ${LARGEST_LOG}; return { rows: log.querySelectorAll("[data-chat-window-row]").length, scrollTop: Math.round(log.scrollTop), scrollHeight: log.scrollHeight }; })()`;
const rowCount = (selector) => page.evaluate(`document.querySelectorAll(${JSON.stringify(selector)}).length`);

async function measure(label, interaction) {
  await page.evaluate(RESET_RENDERS);
  const started = performance.now();
  const detail = await interaction();
  await frame();
  const elapsed = performance.now() - started;
  const renders = await page.evaluate(READ_RENDERS);
  return { label, elapsedMs: Math.round(elapsed), ...renders, detail };
}

const SCENARIOS = {
  "transcript-scroll": async () => {
    const tabId = option("--tab");
    if (tabId) await activateTab(tabId);
    for (let page = 0; page < Number(option("--older") ?? 0); page++) {
      if (!(await rect(`${VISIBLE_LOG} [data-testid=chat-load-older]`))) break;
      await clickSelector(`${VISIBLE_LOG} [data-testid=chat-load-older]`);
      await settle(1200);
    }
    const log = await page.evaluate(`(() => { const log = ${LARGEST_LOG}; const r = log.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    const strokes = Number(option("--strokes") ?? 40);
    return measure(`transcript wheel up ${strokes} + down ${strokes}`, async () => {
      const before = await page.evaluate(TRANSCRIPT_STATE);
      await wheel(log, -240, strokes);
      await settle(300);
      const middle = await page.evaluate(TRANSCRIPT_STATE);
      await wheel(log, 240, strokes);
      await settle(300);
      return { before, middle, after: await page.evaluate(TRANSCRIPT_STATE) };
    });
  },
  "sidebar-scroll": async () => {
    if (!(await rect(".ct-sidebar-content"))) { await clickSelector(`[aria-label="Open sidebar"]`); await settle(800); }
    const sidebar = await need(".ct-sidebar-content");
    return measure("sidebar wheel down 20 + up 20", async () => {
      await wheel(sidebar, 120, 20);
      await settle(300);
      await wheel(sidebar, -120, 20);
      await settle(300);
      return { rows: await rowCount(".ct-sidebar-content [data-tab-id]") };
    });
  },
  "sidebar-hover": async () => {
    const sidebar = await need(".ct-sidebar-content");
    return measure("pointer sweeps sidebar rows twice", async () => {
      for (let pass = 0; pass < 2; pass++) {
        for (let y = sidebar.top + 10; y < sidebar.top + sidebar.height - 10; y += 8) {
          await mouse("mouseMoved", sidebar.left + sidebar.width / 2, y);
          await sleep(16);
        }
      }
      return { rows: await rowCount(".ct-sidebar-content [data-tab-id]") };
    });
  },
  "stream-idle": async () => measure("no input for 5s while agents stream", async () => { await sleep(5000); return {}; }),
  "mesh-click": async () => {
    await openView(`[data-testid=nav-mesh]`, "[data-peer-card]");
    const peers = await page.evaluate(`[...document.querySelectorAll("[data-peer-card] [aria-label^='Open ']")].map((element) => element.getAttribute("aria-label"))`);
    const target = option("--peer") ? `Open ${option("--peer")}` : peers.at(-1);
    const result = await measure(`click ${target}, drawer frames`, async () => {
      const box = await need(`[data-peer-card] [aria-label="${target}"]`);
      await click(box.x, box.y);
      const frames = await framesUntil("[data-testid=mesh-peer-drawer]");
      await settle(400);
      return { peers: peers.length, framesToDrawer: frames };
    });
    // The drawer's own close button; Escape would also leave the Mesh view.
    if (!args.includes("--keep")) await clickSelector(`[data-testid=mesh-peer-drawer] button[title^="Close"]`);
    await settle(300);
    return result;
  },
  "mesh-refresh": async () => {
    await openView(`[data-testid=nav-mesh]`, "[data-peer-card]");
    // The graph toolbar fades in under the pointer.
    const canvas = await need("[data-peer-card]");
    await mouse("mouseMoved", canvas.x, canvas.y);
    await settle(400);
    return measure("press Refresh, wait 3s for the refresh cycle", async () => {
      await clickSelector(`[aria-label="Mesh graph controls"] [aria-label="Refresh"]`);
      await settle(3000);
      return {};
    });
  },
  "memory-graph": async () => {
    await openView("[data-testid=nav-memory]", "[data-testid=memory-tab-inspect]");
    await clickSelector("[data-testid=memory-tab-inspect]");
    await settle(1500);
    const graph = await need("main canvas, main svg");
    return measure("wheel zoom in 15 + out 15, drag pan", async () => {
      const center = { x: graph.x, y: graph.y };
      await wheel(center, -120, 15);
      await wheel(center, 120, 15);
      await mouse("mousePressed", center.x, center.y, { button: "left", buttons: 1, clickCount: 1 });
      for (let step = 1; step <= 20; step++) { await mouse("mouseMoved", center.x + step * 6, center.y + step * 3, { buttons: 1 }); await sleep(16); }
      await mouse("mouseReleased", center.x + 120, center.y + 60, { button: "left", buttons: 0, clickCount: 1 });
      await settle(300);
      return {};
    });
  },
  "memory-list": async () => {
    await openView("[data-testid=nav-memory]", "[data-testid=memory-tab-inspect]");
    await clickSelector("[data-testid=memory-tab-inspect]");
    await settle(800);
    if (!(await rect("[data-testid=memory-list-results]"))) await clickSelector(`[aria-label="Memory graph controls"] button`);
    await settle(1500);
    const list = await need("[data-testid=memory-list-results]");
    return measure("memory list wheel down 20 + up 20", async () => {
      await wheel(list, 120, 20);
      await settle(200);
      await wheel(list, -120, 20);
      await settle(300);
      return {};
    });
  },
  "plan-scroll": async () => {
    await openPlanView("Board");
    const board = await need("main");
    return measure("plan board wheel down 20 + up 20", async () => {
      await wheel(board, 120, 20);
      await wheel(board, -120, 20);
      await settle(300);
      return {};
    });
  },
  "plan-tasks": async () => {
    await openPlanView("Tasks");
    const list = await need("main");
    return measure("plan task list wheel down 20 + up 20", async () => {
      await wheel(list, 120, 20);
      await wheel(list, -120, 20);
      await settle(300);
      return {};
    });
  },
  "analytics-open": async () => measure("open Analytics, wait 2s, wheel down 15", async () => {
    await clickSelector(`[aria-label="Analytics"]`);
    await settle(2000);
    const view = await need("main");
    await wheel(view, 120, 15);
    await settle(300);
    return {};
  }),
  "notifications-open": async () => measure("open Notifications, wheel down 15 + up 15, close", async () => {
    await clickSelector(`[aria-label="Notifications"]`);
    await settle(800);
    const list = await need("[data-testid=notification-panel]");
    await wheel(list, 120, 15);
    await wheel(list, -120, 15);
    await settle(300);
    await page.call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await page.call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    return {};
  }),
  "settings-tabs": async () => {
    await clickSelector("[data-testid=titlebar-settings]");
    await settle(400);
    const sections = await page.evaluate(`[...document.querySelectorAll("[data-testid^=settings-menu-]")].map((item) => item.dataset.testid)`);
    await closeDrawer();
    return measure(`open ${sections.length} settings sections from the gear menu`, async () => {
      for (const section of sections) {
        await clickSelector("[data-testid=titlebar-settings]");
        await settle(250);
        await clickSelector(`[data-testid="${section}"]`);
        await settle(600);
      }
      return { sections: sections.length };
    });
  },
};

async function openPlanView(tab) {
  if (!(await rect(`[aria-label="Plan view"] [role=tab]`))) {
    await clickSelector("[aria-label^='Open task status for']");
    await settle(2000);
  }
  const box = await page.evaluate(`(() => { const tab = [...document.querySelectorAll('[aria-label="Plan view"] [role=tab]')].find((candidate) => candidate.textContent.trim() === ${JSON.stringify(tab)}); const r = tab.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
  await click(box.x, box.y);
  await settle(1500);
}

async function openView(buttonSelector, readySelector) {
  if (!(await rect(readySelector))) {
    await clickSelector(buttonSelector);
    await page.evaluate(`new Promise((resolve, reject) => { const started = performance.now(); const check = () => {
      if ([...document.querySelectorAll(${JSON.stringify(readySelector)})].some(${SHOWN})) resolve(true);
      else if (performance.now() - started > 15000) reject(new Error("view did not open"));
      else requestAnimationFrame(check); }; check(); })`);
  }
  await settle(1500);
}

function framesUntil(selector) {
  return page.evaluate(`new Promise((resolve) => { let frames = 0; const check = () => {
    if ([...document.querySelectorAll(${JSON.stringify(selector)})].some(${SHOWN})) resolve(frames);
    else if (++frames > 120) resolve(-1);
    else requestAnimationFrame(check); }; check(); })`);
}

async function closeDrawer() {
  await page.call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await page.call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
}

const run = SCENARIOS[scenario];
if (!run) throw new Error(`unknown scenario ${scenario}; known: ${Object.keys(SCENARIOS).join(", ")}`);
// Device emulation lasts only as long as this CDP session, so a phone run applies it here and lets layout settle.
if (option("--width")) {
  await page.call("Emulation.setDeviceMetricsOverride", { width: Number(option("--width")), height: Number(option("--height") ?? 844), deviceScaleFactor: 3, mobile: true });
  await page.call("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
  await settle(2500);
}
// The interface-update toast covers the titlebar tray; dismissing it is setup, not part of any measurement.
if (await rect(`[aria-label="Dismiss interface update"]`)) { await clickSelector(`[aria-label="Dismiss interface update"]`); await settle(300); }
if (option("--workspace") && !option("--tab")) await selectWorkspace(option("--workspace"));
const result = await run();
page.close();
if (args.includes("--json")) console.log(JSON.stringify(result));
else {
  console.log(`== ${scenario}: ${result.label}`);
  console.log(`renders=${result.renders} commits=${result.commits} longTasks=${result.longTasks} longest=${result.longestTask}ms elapsed=${result.elapsedMs}ms ${JSON.stringify(result.detail)}`);
  console.log("triggers:\n  " + result.triggers.join("\n  "));
  const top = Number(option("--top") ?? 25);
  const rendered = Object.entries(result.byName).sort((a, b) => b[1] - a[1]).slice(0, top);
  console.log("rendered:\n  " + rendered.map(([name, count]) => `${count} ${name}`).join("\n  "));
  if (option("--watch")) console.log("watched: " + option("--watch").split(",").map((name) => `${name}=${result.byName[name] ?? 0}`).join(" "));
}
