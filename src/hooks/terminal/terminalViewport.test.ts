import {
  captureTerminalViewport,
  restoreTerminalViewport,
  restoreTerminalViewportAfterQueue,
} from "./terminalViewport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const calls: string[] = [];
const writes: string[] = [];
const callbacks: Array<() => void> = [];
const term = {
  buffer: { active: { type: "normal" as const, viewportY: 140, baseY: 140 } },
  scrollToBottom: () => { calls.push("bottom"); },
  scrollToLine: (line: number) => { calls.push(`line:${line}`); },
  write: (data: string, callback?: () => void) => {
    writes.push(data);
    if (callback) callbacks.push(callback);
  },
};

const following = captureTerminalViewport(term);
assert(following.followOutput, "bottom viewport follows output");
restoreTerminalViewport(term, following);
assert(calls.join(",") === "bottom", "following viewport restores at the bottom");

term.buffer.active.viewportY = 37;
const reading = captureTerminalViewport(term);
assert(!reading.followOutput, "scrolled viewport stops following output");
term.buffer.active.baseY = 20;
restoreTerminalViewport(term, reading);
assert(calls[calls.length - 1] === "line:20", "restoration clamps manual position to retained history");

let current = true;
restoreTerminalViewportAfterQueue(term, following, () => current);
assert(writes[writes.length - 1] === "", "visible restoration waits behind the xterm queue");
assert(calls[calls.length - 1] === "line:20", "restore does not race the queued output");
callbacks.shift()?.();
assert(calls[calls.length - 1] === "bottom", "queued restoration returns a following terminal to the bottom");

current = false;
restoreTerminalViewportAfterQueue(term, following, () => current);
callbacks.shift()?.();
assert(calls[calls.length - 1] === "bottom", "stale visibility restoration is ignored");

console.log("terminal viewport checks passed");
