import { scheduleXtermInit, setXtermFrameScheduler } from "./xtermInitScheduler";

function assert(cond: boolean, label: string) {
  if (!cond) throw new Error(`FAILED: ${label}`);
}

// Deterministic clock: collect pending frame callbacks, flush one at a time.
let frames: Array<() => void> = [];
setXtermFrameScheduler((cb) => {
  frames.push(cb);
});
function tick(): boolean {
  const cb = frames.shift();
  if (!cb) return false;
  cb();
  return true;
}
function reset() {
  frames = [];
}

// One task per frame: three mounts must not all run in a single frame.
(() => {
  reset();
  const order: number[] = [];
  scheduleXtermInit(() => order.push(1));
  scheduleXtermInit(() => order.push(2));
  scheduleXtermInit(() => order.push(3));
  assert(order.length === 0, "nothing runs before a frame ticks");
  tick();
  assert(order.length === 1, "exactly one init runs per frame");
  tick();
  tick();
  assert(order.join(",") === "1,2,3", "all inits run in FIFO order across frames");
  assert(!tick(), "queue drained, no extra frames scheduled");
})();

// Priority jumps the queue: a visible pane inits before earlier hidden ones.
(() => {
  reset();
  const order: string[] = [];
  scheduleXtermInit(() => order.push("hidden-a"));
  scheduleXtermInit(() => order.push("hidden-b"));
  scheduleXtermInit(() => order.push("visible"), { priority: true });
  tick();
  assert(order[0] === "visible", "priority task runs first");
  tick();
  tick();
  assert(order.join(",") === "visible,hidden-a,hidden-b", "hidden Tabs follow in order");
})();

// Cancel before run: an unmounted pane's init is skipped, others still run.
(() => {
  reset();
  const order: number[] = [];
  scheduleXtermInit(() => order.push(1));
  const cancel = scheduleXtermInit(() => order.push(2));
  scheduleXtermInit(() => order.push(3));
  cancel();
  tick();
  tick();
  tick();
  assert(order.join(",") === "1,3", "cancelled init is skipped");
  assert(!tick(), "no dangling frame after cancel drain");
})();

setXtermFrameScheduler(null);
console.log("xterm init scheduler checks passed");
