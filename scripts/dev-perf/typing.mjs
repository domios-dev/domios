// Types into the visible DEV composer like a user and reports keystroke-to-frame latency and React renders.
// With --prompt it measures again while the agent answers that prompt.
//   node scripts/dev-perf/typing.mjs [--prompt "..."] [--port 9223]
import { APP_READY, connectPage, reloadWithScript, sleep } from "./cdp.mjs";
import { READ_RENDERS, RENDER_HOOK_SOURCE, RESET_RENDERS } from "./renderHook.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const prompt = option("--prompt");
const page = await connectPage(option("--port") ? Number(option("--port")) : undefined);

const FOCUS_COMPOSER = `(() => {
  const boxes = [...document.querySelectorAll("textarea, [contenteditable=true]")]
    .filter((element) => element.offsetParent && element.getBoundingClientRect().bottom > innerHeight * 0.6);
  const box = boxes.at(-1);
  if (!box) return false;
  box.focus();
  return true;
})()`;
const ACTIVITY = `[...document.querySelectorAll("[data-testid=agent-state]")].filter((e) => e.offsetParent).map((e) => e.dataset.tabActivity).join(",")`;
const NEXT_FRAME = "new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))";

async function key(type, init) {
  await page.call("Input.dispatchKeyEvent", { type, ...init });
}

async function typeText(text) {
  const latencies = [];
  for (const char of text) {
    const started = performance.now();
    await key("keyDown", { text: char, unmodifiedText: char, key: char });
    await key("keyUp", { key: char });
    await page.evaluate(NEXT_FRAME);
    latencies.push(performance.now() - started);
  }
  latencies.sort((a, b) => a - b);
  const at = (quantile) => latencies[Math.min(latencies.length - 1, Math.floor(quantile * latencies.length))].toFixed(0);
  return `p50=${at(0.5)}ms p90=${at(0.9)}ms max=${latencies.at(-1).toFixed(0)}ms n=${latencies.length}`;
}

async function clearComposer() {
  await key("keyDown", { key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65 });
  await key("keyUp", { key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65 });
  await key("keyDown", { key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8 });
  await key("keyUp", { key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8 });
}

async function measure(label, text) {
  await page.evaluate(FOCUS_COMPOSER);
  await page.evaluate(RESET_RENDERS);
  const typing = await typeText(text);
  const renders = await page.evaluate(READ_RENDERS);
  console.log(`\n== ${label}: ${typing}`);
  console.log(`commits=${renders.commits} in ${renders.seconds}s`);
  console.log("triggers:\n  " + renders.triggers.join("\n  "));
  console.log("rendered:\n  " + renders.rendered.join("\n  "));
  await clearComposer();
}

if (!(await page.evaluate("!!window.__devPerfRenders"))) await reloadWithScript(page, RENDER_HOOK_SOURCE, APP_READY);
if (!(await page.evaluate(FOCUS_COMPOSER))) throw new Error("No visible composer; activate an agent tab first");

await measure("idle", "typing at rest to measure input latency ");
if (prompt) {
  await page.evaluate(FOCUS_COMPOSER);
  await typeText(prompt);
  await key("keyDown", { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" });
  await key("keyUp", { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  for (let attempt = 0; attempt < 30 && !(await page.evaluate(ACTIVITY)).includes("working"); attempt++) await sleep(500);
  console.log(`\nactivity after send: ${await page.evaluate(ACTIVITY)}`);
  await sleep(1500);
  await measure("while the agent works", "typing while the agent answers to measure input latency under load ");
}
page.close();
