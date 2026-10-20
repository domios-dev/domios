// Runs several real drags in a row in the General section and checks each against the expected order,
// in the window and on the server. Reproduces "after a few drags it lands in the wrong place".
//   node scripts/dev-perf/dragSeries.mjs [--rounds 6]
import { connectPage, sleep } from "./cdp.mjs";

const args = process.argv.slice(2);
const rounds = Number(args[args.indexOf("--rounds") + 1] || 6);
const page = await connectPage();

const ROWS = `[...document.querySelectorAll("[data-testid=general-section] [data-dnd-group^='tab:'] > [data-tab-id], [data-testid=general-section] [data-dnd-group^='tab:'] [data-tab-id]")]
  .filter((row, index, all) => all.findIndex((other) => other.getAttribute("data-tab-id") === row.getAttribute("data-tab-id")) === index)
  .map((row) => ({ id: row.getAttribute("data-tab-id"), title: row.innerText.split("\\n")[0], rect: (() => { const r = row.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2, h: r.height }; })(), pinned: !!row.closest("[data-dnd-disabled='true']") }))`;
const order = async () => (await page.evaluate(ROWS)).filter((row) => row.rect.h >= 40);
const mouse = (type, x, y, buttons = 1) => page.call("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons, clickCount: type === "mouseMoved" ? 0 : 1, pointerType: "mouse" });

async function drag(from, to) {
  await mouse("mousePressed", from.x, from.y);
  await mouse("mouseMoved", from.x, from.y + (to.y > from.y ? 6 : -6));
  const targetY = to.y + (to.y > from.y ? to.h / 4 : -to.h / 4);
  for (let step = 1; step <= 14; step++) {
    await mouse("mouseMoved", from.x, from.y + ((targetY - from.y) * step) / 14);
    await sleep(16);
  }
  await sleep(120);
  await mouse("mouseReleased", from.x, targetY, 0);
}

const titles = (rows) => rows.map((row) => row.title).join(" | ");
let failures = 0;
for (let round = 0; round < rounds; round++) {
  const before = await order();
  const movable = before.filter((row) => !row.pinned);
  const source = movable[(round * 2) % movable.length];
  const target = movable[(round * 3 + 2) % movable.length];
  if (source.id === target.id) continue;
  const expected = before.map((row) => row.id).filter((id) => id !== source.id);
  const fromIndex = before.findIndex((row) => row.id === source.id);
  const toIndex = before.findIndex((row) => row.id === target.id);
  expected.splice(toIndex, 0, source.id);
  await drag(source.rect, target.rect);
  await sleep(900);
  const after = await order();
  const ok = after.map((row) => row.id).join() === expected.join();
  if (!ok) failures++;
  console.log(`#${round + 1} ${source.title} (${fromIndex}) -> over ${target.title} (${toIndex}): ${ok ? "ok" : "WRONG"}\n  got:      ${titles(after)}${ok ? "" : `\n  expected: ${expected.map((id) => before.find((row) => row.id === id).title).join(" | ")}`}`);
}
console.log(failures ? `${failures} wrong drop(s)` : "all drops landed where expected");
page.close();
