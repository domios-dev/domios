// Drags a General row up and down several times, then checks the drop against what was on screen just
// before release: "what you see is what you get". Also samples the dragged row's position through the
// drop to catch it jumping before it settles.
//   node scripts/dev-perf/dragWiggle.mjs [--rounds 5]
import { connectPage, sleep } from "./cdp.mjs";

const args = process.argv.slice(2);
const rounds = Number(args[args.indexOf("--rounds") + 1] || 5);
const page = await connectPage();
await page.evaluate(`document.querySelector(".ct-sidebar-content").scrollTop = 0`);

const VISUAL_ORDER = `(() => {
  const units = [...document.querySelectorAll("[data-testid=general-section] [data-dnd-group^='tab:']")];
  return units.map((unit) => {
    const row = unit.querySelector("[data-tab-id]");
    const r = unit.getBoundingClientRect();
    return { id: row?.getAttribute("data-tab-id"), title: row?.innerText.split("\\n")[0], top: r.top, x: r.left + r.width / 2, y: r.top + 22, h: r.height, pinned: unit.getAttribute("data-dnd-disabled") === "true" };
  }).filter((unit) => unit.id).sort((a, b) => a.top - b.top);
})()`;
const DRAGGED_TOP = `(() => { const d = document.querySelector("[data-testid=general-section] [data-dnd-dragging='true']"); return d ? d.getBoundingClientRect().top : null; })()`;
const mouse = (type, x, y, buttons = 1) => page.call("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons, clickCount: type === "mouseMoved" ? 0 : 1, pointerType: "mouse" });
const titles = (units) => units.map((unit) => unit.title).join(" | ");

let failures = 0;
for (let round = 0; round < rounds; round++) {
  const units = await page.evaluate(VISUAL_ORDER);
  const movable = units.filter((unit) => !unit.pinned);
  const source = movable[(round * 3 + 1) % movable.length];
  const top = movable[0].y;
  const bottom = movable.at(-1).y + 10;
  await mouse("mousePressed", source.x, source.y);
  await mouse("mouseMoved", source.x, source.y + 8);
  let y = source.y + 8;
  const stops = [bottom, top, bottom - 30, top + 40, (top + bottom) / 2 + round * 20];
  for (const stop of stops) {
    for (let step = 1; step <= 8; step++) {
      await mouse("mouseMoved", source.x, y + ((stop - y) * step) / 8);
      await sleep(12);
    }
    y = stop;
  }
  await sleep(150);
  const shown = (await page.evaluate(VISUAL_ORDER)).map((unit) => unit.id);
  const samples = [];
  await mouse("mouseReleased", source.x, y, 0);
  for (let frame = 0; frame < 12; frame++) {
    samples.push(await page.evaluate(`(() => { const row = document.querySelector('[data-testid=general-section] [data-tab-id="${source.id}"]'); return row ? Math.round(row.getBoundingClientRect().top) : null; })()`));
    await sleep(25);
  }
  await sleep(700);
  const after = await page.evaluate(VISUAL_ORDER);
  const ok = after.map((unit) => unit.id).join() === shown.join();
  const finalTop = samples.at(-1);
  const overshoot = Math.max(...samples.map((top) => Math.abs(top - finalTop)));
  if (!ok) failures++;
  console.log(`#${round + 1} ${source.title}: ${ok ? "landed as shown" : "WRONG"}; drop motion max offset ${overshoot}px [${samples.join(",")}]`);
  if (!ok) console.log(`  shown: ${shown.map((id) => units.find((unit) => unit.id === id)?.title).join(" | ")}\n  got:   ${titles(after)}`);
}
console.log(failures ? `${failures} drop(s) differ from what was shown` : "every drop matched what was shown");
page.close();
