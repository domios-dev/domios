// Drags one sidebar row onto another with real pointer events and reports the row order before and after.
//   node scripts/dev-perf/drag.mjs <source-tab-id> <target-tab-id> [--port 9223]
import { connectPage, sleep } from "./cdp.mjs";

const [sourceId, targetId] = process.argv.slice(2).filter((arg) => !arg.startsWith("--"));
if (!sourceId || !targetId) throw new Error("usage: drag.mjs <source-tab-id> <target-tab-id>");
const page = await connectPage();

const ROW_ORDER = `[...document.querySelectorAll("[data-testid=general-section] [data-tab-id], [data-testid=workspace-detail] [data-tab-id]")]
  .map((row) => row.getAttribute("data-tab-id"))`;
const center = (id) => page.evaluate(`(() => {
  const row = document.querySelector('[data-tab-id="${id}"]');
  if (!row) return null;
  const rect = row.getBoundingClientRect();
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, height: rect.height };
})()`);
const mouse = (type, x, y, buttons = 1) => page.call("Input.dispatchMouseEvent", {
  type, x, y, button: "left", buttons, clickCount: type === "mouseMoved" ? 0 : 1, pointerType: "mouse",
});

const before = await page.evaluate(ROW_ORDER);
const from = await center(sourceId);
const to = await center(targetId);
if (!from || !to) throw new Error("source or target row is not rendered");

// Past the sensor's activation distance first, then glide so the sortable strategy sees intermediate positions.
await mouse("mousePressed", from.x, from.y);
await mouse("mouseMoved", from.x, from.y + 6);
const steps = 16;
const targetY = to.y - to.height / 4;
for (let step = 1; step <= steps; step++) {
  await mouse("mouseMoved", from.x, from.y + 6 + ((targetY - from.y - 6) * step) / steps);
  await sleep(16);
}
await sleep(120);
await mouse("mouseReleased", from.x, targetY, 0);
await sleep(600);

const after = await page.evaluate(ROW_ORDER);
console.log("before:", before.join(" "));
console.log("after: ", after.join(" "));
console.log(before.join() === after.join() ? "order unchanged" : "order changed");
page.close();
